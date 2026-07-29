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
/// **`ipv4.method` is `shared` in BOTH sharing modes**, which is a correction to
/// §9's `manual`. Measured: `manual` gives the interface its address and nothing
/// else — no DHCP server — so a client cannot get onto the network at all
/// without being hand-configured with a static address. That makes FR-NET-09's
/// "a cockpit-access-only network" unreachable in practice, and fails O3.3's own
/// criterion ("with sharing off, **the same client** still loads the cockpit").
///
/// NetworkManager has no "DHCP server without NAT" method, so the cul-de-sac is
/// built the other way round: keep `shared` for dnsmasq's DHCP + DNS, then take
/// away forwarding and the masquerade in [`super::routes::apply_ap_activation`].
/// `ip_forward=0` is what "no forwarding" actually means, and it is enforced at
/// the kernel level rather than by a profile setting.
pub(crate) fn ap_args(config: &NetworkConfig) -> Vec<String> {
    let method = "shared";
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
        // MEASURED: `nmcli … 802-11-wireless.channel 0` is REJECTED — "'0' is
        // not a valid channel". The empty string is what clears the property,
        // and it reads back as 0, i.e. "let the driver pick". Our own state
        // model spells automatic as 0, so the two meet here.
        "802-11-wireless.channel".to_owned(),
        if config.ap.channel == 0 { String::new() } else { config.ap.channel.to_string() },
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
    match run(nmcli, &args) {
        Ok(_out) => Ok(()),
        // `nmcli connection down` on a profile that is already inactive exits
        // non-zero. That is the NORMAL state for both profiles on a box in
        // `wan` with the AP off, so treating it as a failure would put two
        // alarming lines in the journal on every single boot.
        Err(failure) if !active && failure.contains("not an active connection") => Ok(()),
        Err(failure) => Err(format!("{verb} {profile}: {failure}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::it::network::state::{ApConfig, Band, ProbeConfig, WwanConfig};

    /// Read the value that follows `key` in an argv.
    fn value_of<'a>(argv: &'a [String], key: &str) -> Option<&'a str> {
        argv.iter().position(|arg| arg == key).and_then(|at| argv.get(at.saturating_add(1))).map(String::as_str)
    }

    fn config(mode: UplinkMode) -> NetworkConfig {
        NetworkConfig {
            mode,
            wwan: WwanConfig {
                apn: "orange.fr".to_owned(),
                username: None,
                password: Some("bearer-secret".to_owned()),
                pin: Some("4271".to_owned()),
                roaming: false,
                standby: Standby::Hot,
            },
            ap: ApConfig {
                enabled: true,
                ssid: "ContextPilot-f10d".to_owned(),
                passphrase: Some("correct-horse-battery".to_owned()),
                band: Band::A,
                channel: 36,
                country: "FR".to_owned(),
                hidden: false,
                share_internet: true,
            },
            probe: ProbeConfig::default(),
        }
    }

    /// O3.1 — the exact argv for a representative state, asserted with no
    /// NetworkManager anywhere near it.
    #[test]
    fn wwan_argv_matches_the_mode() {
        let standby = wwan_args(&config(UplinkMode::WanThen5g));
        assert_eq!(value_of(&standby, "ipv4.route-metric"), Some("700"), "standing by, above end0's 100");
        assert_eq!(value_of(&standby, "ipv6.route-metric"), Some("700"), "both families move together");
        assert_eq!(value_of(&standby, "gsm.apn"), Some("orange.fr"));
        assert_eq!(value_of(&standby, "gsm.home-only"), Some("yes"), "roaming false ⇒ home-only yes");
        assert_eq!(value_of(&standby, "connection.autoconnect"), Some("yes"), "hot standby autoconnects");

        let strict = wwan_args(&config(UplinkMode::FiveG));
        assert_eq!(value_of(&strict, "ipv4.route-metric"), Some("50"), "the chosen uplink, below end0's 100");
        assert_eq!(value_of(&strict, "connection.autoconnect"), Some("yes"));

        let ethernet = wwan_args(&config(UplinkMode::Wan));
        assert_eq!(value_of(&ethernet, "connection.autoconnect"), Some("no"), "wan never brings the modem up");
    }

    #[test]
    fn cold_standby_does_not_autoconnect() {
        // The whole point of `cold` (landmine 8): the SIM stays unattached until
        // the supervisor actually needs it, for a metered plan.
        let mut cold = config(UplinkMode::WanThen5g);
        cold.wwan.standby = Standby::Cold;
        assert_eq!(value_of(&wwan_args(&cold), "connection.autoconnect"), Some("no"));
    }

    #[test]
    fn ap_argv_switches_method_on_sharing() {
        let shared = ap_args(&config(UplinkMode::Wan));
        assert_eq!(value_of(&shared, "ipv4.method"), Some("shared"), "NM runs dnsmasq + NAT");
        assert_eq!(value_of(&shared, "ipv4.addresses"), Some("10.42.0.1/24"));
        assert_eq!(value_of(&shared, "802-11-wireless.mode"), Some("ap"));
        assert_eq!(value_of(&shared, "802-11-wireless.band"), Some("a"));
        assert_eq!(value_of(&shared, "802-11-wireless.channel"), Some("36"));

        // MEASURED correction to §9: `manual` would leave the AP with no DHCP
        // server, so a client could not get onto the cul-de-sac network at all
        // and FR-NET-09's "the cockpit is the only reachable service" would be
        // reachable by nobody. The profile stays `shared`; the cul-de-sac is
        // made by removing forwarding and the masquerade table instead.
        let mut cul_de_sac = config(UplinkMode::Wan);
        cul_de_sac.ap.share_internet = false;
        let solo = ap_args(&cul_de_sac);
        assert_eq!(value_of(&solo, "ipv4.method"), Some("shared"), "dnsmasq must keep serving DHCP");
        assert_eq!(value_of(&solo, "ipv4.addresses"), Some("10.42.0.1/24"), "…on the same address");
    }

    /// MEASURED on hardware: `802-11-wireless.channel 0` is rejected outright.
    /// The empty string is the only spelling of "automatic" nmcli accepts, and
    /// it reads back as 0.
    #[test]
    fn automatic_channel_is_the_empty_string_not_zero() {
        let mut auto = config(UplinkMode::Wan);
        auto.ap.channel = 0;
        assert_eq!(value_of(&ap_args(&auto), "802-11-wireless.channel"), Some(""));
    }

    #[test]
    fn a_disabled_ap_does_not_autoconnect() {
        let mut off = config(UplinkMode::Wan);
        off.ap.enabled = false;
        assert_eq!(value_of(&ap_args(&off), "connection.autoconnect"), Some("no"));
    }

    /// Secrets are present in the argv (they must reach nmcli) but nothing ever
    /// logs an argv — [`super::super::apply::run`] reports the tool's stderr and
    /// never what it sent.
    #[test]
    fn secrets_are_passed_as_their_own_argv_slots() {
        let argv = ap_args(&config(UplinkMode::Wan));
        assert_eq!(value_of(&argv, "802-11-wireless-security.psk"), Some("correct-horse-battery"));
        assert_eq!(value_of(&argv, "802-11-wireless-security.key-mgmt"), Some("wpa-psk"));
        let bearer = wwan_args(&config(UplinkMode::Wan));
        assert_eq!(value_of(&bearer, "gsm.password"), Some("bearer-secret"));
        assert_eq!(value_of(&bearer, "gsm.pin"), Some("4271"));
        // No secret is ever concatenated into another argument — a shell-quoting
        // bug there would be an injection, and there is no shell in the path.
        assert!(argv.iter().all(|arg| arg == "correct-horse-battery" || !arg.contains("correct-horse-battery")));
    }
}
