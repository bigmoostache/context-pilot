//! Unit tests for the network document, its validation and its secret elision.
//!
//! Everything here runs with **no env gates set**, so the applier is inert
//! (NFR-NET-04) and these tests never touch the machine's network — which is the
//! whole reason the gate exists.

use super::state::{ApConfig, Band, NetworkConfig, ProbeConfig, Standby, UplinkMode, WwanConfig};
use super::{state, status};

/// A fully-populated document with both secrets set, so elision has something
/// to fail to hide.
fn populated() -> NetworkConfig {
    NetworkConfig {
        mode: UplinkMode::WanThen5g,
        wwan: WwanConfig {
            apn: "orange.fr".to_owned(),
            username: Some("orange".to_owned()),
            password: Some("s3cr3t-bearer-password".to_owned()),
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

#[test]
fn document_round_trips_through_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = state::network_path(dir.path());
    // A box that has never been seeded reads as the safe default: ethernet only,
    // AP off. Fail-closed, and reachable on the fleet ULA.
    let fresh = state::load(&path);
    assert_eq!(fresh.mode, UplinkMode::Wan, "an unseeded box defaults to wan");
    assert!(!fresh.ap.enabled, "an unseeded box does not broadcast");

    let config = populated();
    state::save(&path, &config).expect("save");
    assert_eq!(state::load(&path), config, "the document round-trips verbatim");
}

#[test]
#[cfg(unix)]
fn saved_document_is_0600() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = state::network_path(dir.path());
    state::save(&path, &populated()).expect("save");
    let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "the file holds the PSK and the SIM PIN");
}

#[test]
fn a_malformed_file_falls_back_to_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = state::network_path(dir.path());
    std::fs::write(&path, b"{ this is not json").expect("write junk");
    assert_eq!(state::load(&path), NetworkConfig::default(), "junk reads as the safe default");
}

#[test]
fn a_tampered_but_parseable_file_falls_back_to_defaults() {
    // Defence-in-depth: tampering already needs root, but this document is fed
    // straight into nmcli argv, so an invalid-yet-parseable file must not reach
    // the applier.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = state::network_path(dir.path());
    let mut bad = populated();
    bad.ap.ssid = "x".repeat(64); // over the 32-byte SSID limit
    let raw = serde_json::to_vec(&bad).expect("serialize");
    std::fs::write(&path, &raw).expect("write");
    assert_eq!(state::load(&path), NetworkConfig::default(), "an invalid document reads as the default");
}

#[test]
fn ssid_and_passphrase_bounds_are_enforced() {
    let mut config = populated();
    config.ap.ssid = String::new();
    assert!(state::validate_ap(&config.ap).is_err(), "an empty SSID is rejected");
    config.ap.ssid = "x".repeat(33);
    assert!(state::validate_ap(&config.ap).is_err(), "a 33-byte SSID is rejected");
    config.ap.ssid = "ok".to_owned();
    config.ap.passphrase = Some("short".to_owned());
    assert!(state::validate_ap(&config.ap).is_err(), "a 5-char PSK is rejected");
    config.ap.passphrase = Some("x".repeat(64));
    assert!(state::validate_ap(&config.ap).is_err(), "a 64-char PSK is rejected");
    config.ap.passphrase = Some("x".repeat(63));
    assert!(state::validate_ap(&config.ap).is_ok(), "63 characters is the top of the range");
}

#[test]
fn an_ap_cannot_be_enabled_without_a_country() {
    // FR-NET-14: with the world-default domain every 5 GHz channel is `no IR`
    // and the AP simply never beacons, so this is a functional prerequisite
    // rather than a policy check.
    let mut config = populated();
    config.ap.country = String::new();
    assert!(state::validate_ap(&config.ap).is_err(), "enabling with no country is refused");
    config.ap.enabled = false;
    assert!(state::validate_ap(&config.ap).is_ok(), "but a disabled AP may still be saved without one");
}

#[test]
fn an_ap_cannot_be_enabled_without_a_passphrase() {
    let mut config = populated();
    config.ap.passphrase = None;
    assert!(state::validate_ap(&config.ap).is_err(), "we do not ship open networks");
}

#[test]
fn country_must_be_two_letters() {
    let mut config = populated();
    config.ap.country = "FRA".to_owned();
    assert!(state::validate_ap(&config.ap).is_err(), "three letters is not ISO-3166 alpha-2");
    config.ap.country = "F1".to_owned();
    assert!(state::validate_ap(&config.ap).is_err(), "digits are not a country");
}

#[test]
fn channel_must_be_valid_for_the_band() {
    let mut config = populated();
    config.ap.band = Band::A;
    config.ap.channel = 6;
    assert!(state::validate_ap(&config.ap).is_err(), "channel 6 is 2.4 GHz, not 5");
    config.ap.channel = 0;
    assert!(state::validate_ap(&config.ap).is_ok(), "0 is automatic on every band");
    config.ap.band = Band::Bg;
    config.ap.channel = 36;
    assert!(state::validate_ap(&config.ap).is_err(), "channel 36 is 5 GHz, not 2.4");
    config.ap.channel = 11;
    assert!(state::validate_ap(&config.ap).is_ok(), "channel 11 is 2.4 GHz");
}

#[test]
fn apn_and_pin_charsets_are_enforced() {
    let mut config = populated();
    config.wwan.apn = "orange fr; rm -rf /".to_owned();
    assert!(state::validate_wwan(&config.wwan).is_err(), "an APN with spaces/semicolons is rejected");
    config.wwan.apn = "internet.sfr".to_owned();
    assert!(state::validate_wwan(&config.wwan).is_ok());
    config.wwan.pin = Some("12".to_owned());
    assert!(state::validate_wwan(&config.wwan).is_err(), "a 2-digit PIN is rejected");
    config.wwan.pin = Some("abcd".to_owned());
    assert!(state::validate_wwan(&config.wwan).is_err(), "a non-numeric PIN would burn an unlock retry");
    config.wwan.pin = Some("00000000".to_owned());
    assert!(state::validate_wwan(&config.wwan).is_ok(), "8 digits is the top of the range");
}

/// FR-NET-13 — the load-bearing test. Every read-path projection is serialised
/// and searched for the literal secrets. If a field is ever added to the struct
/// and mirrored into `redacted` without thought, this is what catches it.
#[test]
fn no_read_path_output_contains_a_secret() {
    let config = populated();
    let projections = [
        serde_json::to_string(&config.redacted()).expect("serialize redacted"),
        serde_json::to_string(&config.redacted_ap()).expect("serialize ap"),
        serde_json::to_string(&config.redacted_wwan()).expect("serialize wwan"),
    ];
    for body in &projections {
        assert!(!body.contains("correct-horse-battery"), "the Wi-Fi PSK leaked: {body}");
        assert!(!body.contains("s3cr3t-bearer-password"), "the bearer password leaked: {body}");
        assert!(!body.contains("4271"), "the SIM PIN leaked: {body}");
    }
    // …and the booleans that replace them are present and true.
    let full = &projections[0];
    assert!(full.contains("\"passphrase_set\":true"), "the UI is told a PSK exists");
    assert!(full.contains("\"pin_set\":true"), "the UI is told a PIN exists");
    assert!(full.contains("\"password_set\":true"), "the UI is told a bearer password exists");
}

#[test]
fn probe_tuning_is_bounded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = state::network_path(dir.path());
    let mut config = populated();
    config.probe.targets = vec![];
    let raw = serde_json::to_vec(&config).expect("serialize");
    std::fs::write(&path, &raw).expect("write");
    assert_eq!(state::load(&path), NetworkConfig::default(), "no probe target is not a usable supervisor config");

    config.probe.targets = vec!["not-an-ip".to_owned()];
    let raw = serde_json::to_vec(&config).expect("serialize");
    std::fs::write(&path, &raw).expect("write");
    assert_eq!(state::load(&path), NetworkConfig::default(), "probe targets must be addresses");
}

#[test]
fn mode_spellings_match_the_wire() {
    assert_eq!(UplinkMode::Wan.as_str(), "wan");
    assert_eq!(UplinkMode::WanThen5g.as_str(), "wan_5g");
    assert_eq!(UplinkMode::FiveG.as_str(), "5g");
    // The JSON spelling and `as_str` must not drift — the supervisor's env file
    // is written from one and the state file from the other.
    for mode in [UplinkMode::Wan, UplinkMode::WanThen5g, UplinkMode::FiveG] {
        let json = serde_json::to_string(&mode).expect("serialize mode");
        assert_eq!(json, format!("\"{}\"", mode.as_str()), "as_str tracks the serde rename");
    }
}

#[test]
fn status_degrades_to_null_for_every_gated_field() {
    // O3.5's off-box half. The two halves that need a TOOL degrade to null; the
    // two that only need `/proc/net/route` stay truthful, because they are what
    // an admin watches during a failover and a box missing every optional tool
    // still deserves an honest answer there.
    let status = status::probe(&NetworkConfig::default());
    assert!(status.get("wwan").is_some_and(serde_json::Value::is_null), "no CP_MMCLI_BIN ⇒ null bearer");
    assert!(status.get("ap").is_some_and(serde_json::Value::is_null), "no CP_NMCLI_BIN ⇒ null AP");
    assert!(status.get("active_uplink").is_some_and(|v| v.is_string()), "the active uplink needs no tool");
    let wan = status.get("wan").expect("wan is always present");
    assert!(wan.get("has_default_route").is_some_and(serde_json::Value::is_boolean), "read from /proc/net/route");
    assert!(wan.get("ip").is_some_and(serde_json::Value::is_null), "no CP_IP_BIN ⇒ null address");
}
