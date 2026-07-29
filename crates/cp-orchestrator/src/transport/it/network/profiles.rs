//! The two NetworkManager profiles — `cp-wwan` (the 5G bearer) and `cp-ap`
//! (the access point) — rendered from [`NetworkConfig`].
//!
//! # Why NetworkManager at all
//!
//! ModemManager does not configure interfaces — that is the connection
//! manager's job, and systemd-networkd has no ModemManager integration. Going
//! without NM means hand-writing bearer setup, IP application, DNS merge and
//! reconnect/backoff, plus hostapd, plus a DHCP server, plus nftables NAT. NM
//! ships all of it declaratively, driven by a CLI the backend can call. What it
//! must **never** do is touch the ethernet — see the seam in
//! `deploy/photonicat/network/10-cp-unmanaged.conf`.
//!
//! # Secrets
//!
//! The PSK, the bearer password and the SIM PIN are passed to `nmcli` as ordinary
//! argv slots and are never logged: [`super::apply::run`] reports the tool's own
//! stderr and never the argv it sent (O3.1).

use std::ffi::OsStr;

use super::apply::{AP_PROFILE, WWAN_PROFILE, ap_device, run, wwan_device};
use super::state::{NetworkConfig, Standby, UplinkMode};

/// `nmcli`'s spelling of a boolean.
const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// Whether `name` is a connection NetworkManager already knows.
fn exists(nmcli: &OsStr, name: &str) -> bool {
    run(nmcli, &["-t".to_owned(), "-f".to_owned(), "NAME".to_owned(), "connection".to_owned(), "show".to_owned()])
        .is_ok_and(|out| out.lines().any(|line| line == name))
}

/// Route metric for `cp-wwan` in the given mode: preferred in strict `5g`,
/// standby otherwise. `end0` sits at 100 (netplan's DHCP default), so 50 wins
/// and 700 loses — the two stacks meet only in the kernel routing table, where a
/// metric is a metric regardless of who installed it.
pub(crate) const fn wwan_metric(mode: UplinkMode) -> u32 {
    match mode {
        UplinkMode::FiveG => super::apply::METRIC_PREFERRED,
        UplinkMode::Wan | UplinkMode::WanThen5g => super::apply::METRIC_STANDBY,
    }
}

/// Whether `cp-wwan` should bring itself up on its own.
///
/// `wan` never. Strict `5g` always — it is the only uplink. `wan_5g` only in
/// `hot` standby: `cold` deliberately leaves the bearer down and lets the
/// supervisor pay the setup cost at failover time, for metered SIMs (landmine 8).
const fn wwan_autoconnect(config: &NetworkConfig) -> bool {
    match config.mode {
        UplinkMode::Wan => false,
        UplinkMode::FiveG => true,
        UplinkMode::WanThen5g => matches!(config.wwan.standby, Standby::Hot),
    }
}

/// The full `nmcli connection modify cp-wwan …` argument vector.
///
/// Split out from [`reconcile_wwan`] so a unit test can assert the exact argv
/// for a representative state without a NetworkManager anywhere near it (O3.1).
pub(crate) fn wwan_args(config: &NetworkConfig) -> Vec<String> {
    let metric = wwan_metric(config.mode).to_string();
    vec![
        "connection".to_owned(),
        "modify".to_owned(),
        WWAN_PROFILE.to_owned(),
        "connection.interface-name".to_owned(),
        wwan_device(),
        "gsm.apn".to_owned(),
        config.wwan.apn.clone(),
        "gsm.username".to_owned(),
        config.wwan.username.clone().unwrap_or_default(),
        "gsm.password".to_owned(),
        config.wwan.password.clone().unwrap_or_default(),
        "gsm.pin".to_owned(),
        config.wwan.pin.clone().unwrap_or_default(),
        // `home-only yes` is NM's way of spelling "do not roam".
        "gsm.home-only".to_owned(),
        yes_no(!config.wwan.roaming).to_owned(),
        "connection.autoconnect".to_owned(),
        yes_no(wwan_autoconnect(config)).to_owned(),
        "ipv4.route-metric".to_owned(),
        metric.clone(),
        "ipv6.route-metric".to_owned(),
        metric,
    ]
}

/// The full `nmcli connection modify cp-ap …` argument vector.
///
/// `ipv4.method shared` is what makes NetworkManager run dnsmasq for DHCP + DNS
/// on the AP subnet and install the masquerade through its nftables backend;
/// `manual` on the same address leaves the AP a cul-de-sac with no forwarding
/// and no NAT, whose only reachable service is the cockpit (FR-NET-09).
pub(crate) fn ap_args(config: &NetworkConfig) -> Vec<String> {
    let method = if config.ap.share_internet { "shared" } else { "manual" };
    vec![
        "connection".to_owned(),
        "modify".to_owned(),
        AP_PROFILE.to_owned(),
        "connection.interface-name".to_owned(),
        ap_device(),
        "802-11-wireless.mode".to_owned(),
        "ap".to_owned(),
        "802-11-wireless.ssid".to_owned(),
        config.ap.ssid.clone(),
        "802-11-wireless.band".to_owned(),
        config.ap.band.as_str().to_owned(),
        // 0 is nmcli's "let the driver pick".
        "802-11-wireless.channel".to_owned(),
        config.ap.channel.to_string(),
        "802-11-wireless.hidden".to_owned(),
        yes_no(config.ap.hidden).to_owned(),
        "802-11-wireless-security.key-mgmt".to_owned(),
        "wpa-psk".to_owned(),
        "802-11-wireless-security.psk".to_owned(),
        config.ap.passphrase.clone().unwrap_or_default(),
        "ipv4.method".to_owned(),
        method.to_owned(),
        "ipv4.addresses".to_owned(),
        format!("{}/24", super::apply::AP_ADDRESS),
        "connection.autoconnect".to_owned(),
        yes_no(config.ap.enabled).to_owned(),
    ]
}

/// Create `cp-wwan` if absent, then push the rendered settings.
///
/// # Errors
///
/// Returns `nmcli`'s stderr when the profile cannot be created or modified.
pub(crate) fn reconcile_wwan(nmcli: &OsStr, config: &NetworkConfig) -> Result<(), String> {
    if !exists(nmcli, WWAN_PROFILE) {
        let created = vec![
            "connection".to_owned(),
            "add".to_owned(),
            "type".to_owned(),
            "gsm".to_owned(),
            "con-name".to_owned(),
            WWAN_PROFILE.to_owned(),
            "ifname".to_owned(),
            wwan_device(),
            "apn".to_owned(),
            config.wwan.apn.clone(),
            // Never autoconnect at creation: the mode step decides, and a
            // half-configured bearer must not race ahead of it.
            "connection.autoconnect".to_owned(),
            "no".to_owned(),
        ];
        let _out = run(nmcli, &created).map_err(|e| format!("create {WWAN_PROFILE}: {e}"))?;
    }
    let _out = run(nmcli, &wwan_args(config)).map_err(|e| format!("configure {WWAN_PROFILE}: {e}"))?;
    Ok(())
}

/// Create `cp-ap` if absent, then push the rendered settings.
///
/// # Errors
///
/// Returns `nmcli`'s stderr when the profile cannot be created or modified.
pub(crate) fn reconcile_ap(nmcli: &OsStr, config: &NetworkConfig) -> Result<(), String> {
    if !exists(nmcli, AP_PROFILE) {
        let created = vec![
            "connection".to_owned(),
            "add".to_owned(),
            "type".to_owned(),
            "wifi".to_owned(),
            "con-name".to_owned(),
            AP_PROFILE.to_owned(),
            "ifname".to_owned(),
            ap_device(),
            "ssid".to_owned(),
            config.ap.ssid.clone(),
            "connection.autoconnect".to_owned(),
            "no".to_owned(),
        ];
        let _out = run(nmcli, &created).map_err(|e| format!("create {AP_PROFILE}: {e}"))?;
    }
    let _out = run(nmcli, &ap_args(config)).map_err(|e| format!("configure {AP_PROFILE}: {e}"))?;
    Ok(())
}

/// Bring a profile up or down, tolerating "already in that state".
///
/// `nmcli connection down` on an inactive profile and `up` on an active one both
/// exit non-zero, and neither is a failure of ours. Activation failures on the
/// bearer are also non-fatal by design — see [`super::apply`].
pub(crate) fn set_active(nmcli: &OsStr, profile: &str, active: bool) -> Result<(), String> {
    let verb = if active { "up" } else { "down" };
    let args = ["connection".to_owned(), verb.to_owned(), profile.to_owned()];
    run(nmcli, &args).map(|_out| ()).map_err(|e| format!("{verb} {profile}: {e}"))
}
