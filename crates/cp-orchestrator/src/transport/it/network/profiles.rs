//! The two `NetworkManager` profiles — `cp-wwan` (the 5G bearer) and `cp-ap`
//! (the access point) — rendered from [`NetworkConfig`].
//!
//! # Why `NetworkManager` at all
//!
//! `ModemManager` does not configure interfaces — that is the connection
//! manager's job, and systemd-networkd has no `ModemManager` integration. Going
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
//! stderr and never the argv it sent.

use std::ffi::OsStr;

use super::apply::{AP_PROFILE, WWAN_PROFILE, ap_device, run, wwan_device};
use super::state::{NetworkConfig, Standby, UplinkMode};

/// `nmcli`'s spelling of a boolean.
const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// Whether `name` is a connection `NetworkManager` already knows.
fn exists(nmcli: &OsStr, name: &str) -> bool {
    run(nmcli, &["-t".to_owned(), "-f".to_owned(), "NAME".to_owned(), "connection".to_owned(), "show".to_owned()])
        .is_ok_and(|out| out.lines().any(|line| line == name))
}

/// Route metric for `cp-wwan` in the given mode: preferred in strict `5g`,
/// standby otherwise. `end0` sits at 100 (netplan's DHCP default), so 50 wins
/// and 700 loses — the two stacks meet only in the kernel routing table, where a
/// metric is a metric regardless of who installed it.
///
/// # Who owns this metric (do not undo)
///
/// * `wan` and `5g` — **the applier**. The metric is a constant of the mode,
///   nothing else moves it, and [`wwan_args`] re-pins it on every apply.
/// * `wan_5g` — **`cp-uplink-watch`**. There the metric *is* the failover
///   mechanism: the supervisor drops the bearer to 50 to promote it and lifts
///   it back to 700 to demote it. So [`wwan_args`] deliberately omits both
///   route-metric properties in that mode and this value is written **once**,
///   in [`reconcile_wwan`]'s `connection add`, as the profile's starting point.
///
/// Pushing it unconditionally meant that saving an SSID during an outage
/// reset the metric the supervisor had just promoted, while the supervisor still
/// believed `promoted=yes` and would therefore never re-promote — a silent loss
/// of the 5G uplink at `NetworkManager`'s next reactivation.
pub(crate) const fn wwan_metric(mode: UplinkMode) -> u32 {
    match mode {
        UplinkMode::FiveG => super::apply::METRIC_PREFERRED,
        UplinkMode::Wan | UplinkMode::WanThen5g => super::apply::METRIC_STANDBY,
    }
}

/// Whether the applier, rather than the supervisor, owns the bearer's route
/// metric in this mode. See [`wwan_metric`] for the boundary.
const fn applier_owns_metric(mode: UplinkMode) -> bool {
    match mode {
        UplinkMode::Wan | UplinkMode::FiveG => true,
        UplinkMode::WanThen5g => false,
    }
}

/// Whether `cp-wwan` should bring itself up on its own.
///
/// `wan` never. Strict `5g` always — it is the only uplink. `wan_5g` only in
/// `hot` standby: `cold` deliberately leaves the bearer down and lets the
/// supervisor pay the setup cost at failover time, for metered SIMs — `hot`
/// keeps the SIM attached and consuming keep-alive data.
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
/// for a representative state without a `NetworkManager` anywhere near it.
///
/// In `wan_5g` the two route-metric properties are **absent** — the supervisor
/// owns them there, and an unrelated save must not walk on a live failover.
/// See [`wwan_metric`] for the ownership boundary.
pub(crate) fn wwan_args(config: &NetworkConfig) -> Vec<String> {
    let mut args = vec![
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
    ];
    if applier_owns_metric(config.mode) {
        let metric = wwan_metric(config.mode).to_string();
        args.push("ipv4.route-metric".to_owned());
        args.push(metric.clone());
        args.push("ipv6.route-metric".to_owned());
        args.push(metric);
    }
    args
}

/// The full `nmcli connection modify cp-ap …` argument vector.
///
/// **`ipv4.method` is `shared` in BOTH sharing modes**, which corrects the
/// `manual` this was first specified with. MEASURED: `manual` gives the
/// interface its address and nothing else — no DHCP server — so a client cannot
/// get onto the network at all without being hand-configured with a static
/// address. That makes the whole point of sharing-off, a cockpit-access-only
/// network, unreachable in practice: with sharing off, **the same client** must
/// still be able to load the cockpit.
///
/// `NetworkManager` has no "DHCP server without NAT" method, so the cul-de-sac is
/// built the other way round: keep `shared` for dnsmasq's DHCP + DNS, then take
/// away forwarding and the masquerade in [`super::routes::apply_ap_activation`].
/// `ip_forward=0` is what "no forwarding" actually means, and it is enforced at
/// the kernel level rather than by a profile setting.
pub(crate) fn ap_args(config: &NetworkConfig) -> Vec<String> {
    let method = "shared";
    let mut args = vec![
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
        "ipv4.method".to_owned(),
        method.to_owned(),
        "ipv4.addresses".to_owned(),
        format!("{}/24", super::apply::AP_ADDRESS),
        "connection.autoconnect".to_owned(),
        yes_no(config.ap.enabled).to_owned(),
    ];
    args.extend(security_args(config));
    args
}

/// The `802-11-wireless-security.*` group — **omitted entirely when no
/// passphrase is set**.
///
/// A factory-fresh box has `passphrase: None` (the Ansible default whenever
/// `ap_password` is empty), and the previous code still sent
/// `802-11-wireless-security.psk ""` alongside `key-mgmt wpa-psk`. If nmcli
/// refuses that pair, [`super::apply::apply`] returns `Err` and **every** network
/// POST on such a box answers `502`, plus a WARN at every boot — an entire class
/// of box bricked for the API by a default. Nothing is lost by leaving the group
/// out: `enabled_ap_prerequisites` already refuses to enable an AP without a
/// passphrase, so the profile simply is not securable until one is set.
///
/// The trade-off, stated so nobody has to rediscover it: clearing a
/// **previously-set** passphrase does not scrub the PSK out of the root-owned
/// `cp-ap` profile — it stays there until a new one replaces it. That is the
/// right side to fail on. The alternative is emitting an empty `psk`, which is
/// exactly the argv that may be rejected, and the AP cannot beacon in either
/// case because the document is what gates activation.
fn security_args(config: &NetworkConfig) -> Vec<String> {
    let Some(psk) = config.ap.passphrase.as_ref() else {
        return Vec::new();
    };
    vec![
        // `wpa-psk` + RSN/CCMP — and this IS how WPA3 ships here. The four
        // settings are one decision, not two.
        //
        // Left at NM's defaults, the AP advertised a legacy WPA1 information
        // element with TKIP alongside an RSN element whose only AKM was `PSK`:
        // deprecated crypto and a downgrade surface, on a network whose whole
        // job is to reach the cockpit. Pinning the protocol to `rsn` and both
        // cipher slots to `ccmp` removes the WPA1 element — and NM 1.52.1 then
        // widens the RSN AKM list on its own. MEASURED beacon, after:
        //
        //     RSN: Group cipher: CCMP · Pairwise ciphers: CCMP
        //          Authentication suites: PSK PSK/SHA-256 SAE
        //
        // That is genuine WPA2/WPA3-Personal transition mode, and a WPA2 client
        // still associates on 5 GHz and takes a DHCP lease.
        //
        // `key-mgmt sae` was the obvious alternative and is the wrong one: it
        // works, but it makes the network WPA3-ONLY, because NM cannot express
        // transition mode that way — `pmf optional` alongside `sae` is refused
        // outright ("pmf can only be 'default' or 'required' when using … 'sae'").
        // That would silently lock out every WPA2-only device on a client's site.
        "802-11-wireless-security.key-mgmt".to_owned(),
        "wpa-psk".to_owned(),
        "802-11-wireless-security.proto".to_owned(),
        "rsn".to_owned(),
        "802-11-wireless-security.pairwise".to_owned(),
        "ccmp".to_owned(),
        "802-11-wireless-security.group".to_owned(),
        "ccmp".to_owned(),
        "802-11-wireless-security.psk".to_owned(),
        psk.clone(),
    ]
}

/// The one-off `nmcli connection add cp-wwan …` argument vector.
///
/// Split out from [`reconcile_wwan`] for the same reason [`wwan_args`] is: it
/// is now the **only** place `wan_5g`'s route metric is ever written, so a test
/// has to be able to see it without a `NetworkManager`.
fn wwan_create_args(config: &NetworkConfig) -> Vec<String> {
    let metric = wwan_metric(config.mode).to_string();
    vec![
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
        // The bearer's STARTING metric. In `wan_5g` this is written here and
        // nowhere else — from then on `cp-uplink-watch` arbitrates it and
        // [`wwan_args`] deliberately leaves it alone. In `wan` and `5g` it
        // is redundant with the `modify` that follows, and harmlessly so.
        "ipv4.route-metric".to_owned(),
        metric.clone(),
        "ipv6.route-metric".to_owned(),
        metric,
    ]
}

/// Create `cp-wwan` if absent, then push the rendered settings.
///
/// # Errors
///
/// Returns `nmcli`'s stderr when the profile cannot be created or modified.
pub(crate) fn reconcile_wwan(nmcli: &OsStr, config: &NetworkConfig) -> Result<(), String> {
    if !exists(nmcli, WWAN_PROFILE) {
        let _out = run(nmcli, &wwan_create_args(config)).map_err(|e| format!("create {WWAN_PROFILE}: {e}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::it::network::state::{ApConfig, Band, ProbeConfig, WwanConfig};

    /// Read the value that follows `key` in an argv.
    fn value_of<'argv>(argv: &'argv [String], key: &str) -> Option<&'argv str> {
        argv.iter().position(|arg| arg == key).and_then(|at| argv.get(at.saturating_add(1))).map(String::as_str)
    }

    /// Assert `key` is present in `argv` with value `want`. A plain call (not a
    /// branch), so hoisting the argv assertions through it keeps the argv tests
    /// under the cognitive-complexity cap.
    fn expect_kv(argv: &[String], key: &str, want: &str) {
        assert_eq!(value_of(argv, key), Some(want), "{key}");
    }

    /// Assert `key` does not appear in `argv`. Companion to [`expect_kv`].
    fn expect_absent(argv: &[String], key: &str) {
        assert_eq!(value_of(argv, key), None, "{key} must be absent");
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

    /// The exact argv for a representative state, asserted with no
    /// `NetworkManager` anywhere near it.
    #[test]
    fn wwan_argv_matches_the_mode() {
        let standby = wwan_args(&config(UplinkMode::WanThen5g));
        expect_kv(&standby, "gsm.apn", "orange.fr");
        expect_kv(&standby, "gsm.home-only", "yes"); // roaming false ⇒ home-only yes
        expect_kv(&standby, "connection.autoconnect", "yes"); // hot standby autoconnects

        let strict = wwan_args(&config(UplinkMode::FiveG));
        expect_kv(&strict, "ipv4.route-metric", "50"); // the chosen uplink, below end0's 100
        expect_kv(&strict, "ipv6.route-metric", "50"); // both families move together
        expect_kv(&strict, "connection.autoconnect", "yes");

        let ethernet = wwan_args(&config(UplinkMode::Wan));
        expect_kv(&ethernet, "ipv4.route-metric", "700"); // parked above end0's 100
        expect_kv(&ethernet, "connection.autoconnect", "no"); // wan never brings the modem up
    }

    /// In `wan_5g` the METRIC IS THE FAILOVER MECHANISM and belongs to
    /// `cp-uplink-watch`. Saving an SSID during an outage used to re-pin it to
    /// 700 while the supervisor still believed `promoted=yes` and would never
    /// re-promote, silently costing the box its 5G uplink.
    #[test]
    fn the_supervisor_owns_the_metric_in_wan_5g() {
        let failover = wwan_args(&config(UplinkMode::WanThen5g));
        expect_absent(&failover, "ipv4.route-metric"); // an unrelated apply must not touch it
        expect_absent(&failover, "ipv6.route-metric"); // …in either family
        assert!(
            failover.iter().all(|arg| !arg.contains("route-metric")),
            "no route-metric property survives in wan_5g: {failover:?}"
        );

        // …but the profile still starts life at the standby metric, written
        // once at creation. Without this the bearer would be created at NM's
        // default and could outrank end0 before the supervisor ever ran.
        let created = wwan_create_args(&config(UplinkMode::WanThen5g));
        expect_kv(&created, "ipv4.route-metric", "700"); // the starting point is still ours
        expect_kv(&created, "ipv6.route-metric", "700");
        expect_kv(&wwan_create_args(&config(UplinkMode::FiveG)), "ipv4.route-metric", "50");
    }

    #[test]
    fn cold_standby_does_not_autoconnect() {
        // The whole point of `cold`: the SIM stays unattached until
        // the supervisor actually needs it, for a metered plan.
        let mut cold = config(UplinkMode::WanThen5g);
        cold.wwan.standby = Standby::Cold;
        assert_eq!(value_of(&wwan_args(&cold), "connection.autoconnect"), Some("no"));
    }

    #[test]
    fn ap_argv_switches_method_on_sharing() {
        let shared = ap_args(&config(UplinkMode::Wan));
        expect_kv(&shared, "ipv4.method", "shared"); // NM runs dnsmasq + NAT
        expect_kv(&shared, "ipv4.addresses", "10.42.0.1/24");
        expect_kv(&shared, "802-11-wireless.mode", "ap");
        expect_kv(&shared, "802-11-wireless.band", "a");
        expect_kv(&shared, "802-11-wireless.channel", "36");

        // MEASURED: `manual` would leave the AP with no DHCP server, so a client
        // could not get onto the cul-de-sac network at all, and the cockpit —
        // the one service that network exists to reach — would be reachable by
        // nobody. The profile stays `shared`; the cul-de-sac is
        // made by removing forwarding and the masquerade table instead.
        let mut cul_de_sac = config(UplinkMode::Wan);
        cul_de_sac.ap.share_internet = false;
        let solo = ap_args(&cul_de_sac);
        expect_kv(&solo, "ipv4.method", "shared"); // dnsmasq must keep serving DHCP
        expect_kv(&solo, "ipv4.addresses", "10.42.0.1/24"); // …on the same address
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

    /// The four settings that together give WPA2/WPA3 transition mode
    /// with no legacy TKIP. Measured beacon: `Authentication suites: PSK
    /// PSK/SHA-256 SAE` over RSN/CCMP, with the WPA1 element gone.
    #[test]
    fn the_ap_advertises_rsn_ccmp_only_with_no_legacy_tkip() {
        let argv = ap_args(&config(UplinkMode::Wan));
        assert_eq!(value_of(&argv, "802-11-wireless-security.key-mgmt"), Some("wpa-psk"));
        assert_eq!(value_of(&argv, "802-11-wireless-security.proto"), Some("rsn"), "no WPA1 element");
        assert_eq!(value_of(&argv, "802-11-wireless-security.pairwise"), Some("ccmp"), "no TKIP");
        assert_eq!(value_of(&argv, "802-11-wireless-security.group"), Some("ccmp"), "no TKIP");
    }

    /// A factory-fresh box has no passphrase, and `psk ""` alongside
    /// `key-mgmt wpa-psk` is an argv nmcli may reject outright. One rejection
    /// there turns **every** network POST on such a box into a `502`, so the
    /// whole security group is omitted until a PSK exists.
    #[test]
    fn no_passphrase_means_no_security_group_at_all() {
        let mut fresh = config(UplinkMode::Wan);
        fresh.ap.enabled = false; // an AP with no PSK cannot be enabled anyway
        fresh.ap.passphrase = None;
        let argv = ap_args(&fresh);
        assert!(
            argv.iter().all(|arg| !arg.starts_with("802-11-wireless-security.")),
            "not one security property is sent without a PSK: {argv:?}"
        );
        assert!(argv.iter().all(|arg| !arg.is_empty()), "and no empty value is sent in its place");
        // The rest of the profile is unaffected — the box still gets its SSID,
        // its band and its address, it simply is not securable yet.
        assert_eq!(value_of(&argv, "802-11-wireless.ssid"), Some("ContextPilot-f10d"));
        assert_eq!(value_of(&argv, "ipv4.addresses"), Some("10.42.0.1/24"));

        // With one set, the whole group comes back.
        assert_eq!(value_of(&ap_args(&config(UplinkMode::Wan)), "802-11-wireless-security.key-mgmt"), Some("wpa-psk"));
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
        let bearer = wwan_args(&config(UplinkMode::Wan));
        assert_eq!(value_of(&bearer, "gsm.password"), Some("bearer-secret"));
        assert_eq!(value_of(&bearer, "gsm.pin"), Some("4271"));
        // No secret is ever concatenated into another argument — a shell-quoting
        // bug there would be an injection, and there is no shell in the path.
        assert!(argv.iter().all(|arg| arg == "correct-horse-battery" || !arg.contains("correct-horse-battery")));
    }
}
