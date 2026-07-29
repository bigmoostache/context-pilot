//! Unit tests for the network document, its validation and its secret elision.
//!
//! Everything here runs with **no env gates set**, so the applier is inert and
//! these tests never touch the machine's network — which is the whole reason the
//! gate exists.

use super::apply::{
    Marks, STEP_AP, STEP_AP_ACTIVATION, STEP_MODE, STEP_UPLINK_ENV, STEP_WWAN, StepHashes, coerce_mode, step,
};
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
    // With the world-default regulatory domain `00` every 5 GHz channel is
    // `no IR` and the AP simply never beacons, so this is a functional
    // prerequisite rather than a policy check.
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

/// The load-bearing secret test. Every read-path projection is serialised
/// and searched for the literal secrets. If a field is ever added to the struct
/// and mirrored into `redacted` without thought, this is what catches it.
#[test]
fn no_read_path_output_contains_a_secret() {
    let config = populated();
    let projections = [
        // `true` is the *worse* case for this test: the bearer block is present,
        // so its secrets have somewhere to leak from.
        serde_json::to_string(&config.redacted(true)).expect("serialize redacted"),
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
    // The off-box half. The two halves that need a TOOL degrade to null; the
    // two that only need `/proc/net/route` stay truthful, because they are what
    // an admin watches during a failover and a box missing every optional tool
    // still deserves an honest answer there.
    let status = status::probe(&NetworkConfig::default(), true);
    assert!(status.get("wwan").is_some_and(serde_json::Value::is_null), "no CP_MMCLI_BIN ⇒ null bearer");
    assert!(status.get("ap").is_some_and(serde_json::Value::is_null), "no CP_NMCLI_BIN ⇒ null AP");
    assert!(status.get("active_uplink").is_some_and(|v| v.is_string()), "the active uplink needs no tool");
    let wan = status.get("wan").expect("wan is always present");
    assert!(wan.get("has_default_route").is_some_and(serde_json::Value::is_boolean), "read from /proc/net/route");
    assert!(wan.get("ip").is_some_and(serde_json::Value::is_null), "no CP_IP_BIN ⇒ null address");
}

// ── The per-step applied marks ──────────────────────────────────────────────

/// The fingerprint used to be whole-document, so **any** mode change
/// re-ran `reconcile_ap` + `nmcli connection up cp-ap` and dropped every
/// associated Wi-Fi client. The comment next to it claimed the opposite.
#[test]
fn a_mode_change_does_not_touch_the_access_points_hash() {
    let ethernet = populated();
    let mut strict = populated();
    strict.mode = UplinkMode::FiveG;
    let (before, after) = (StepHashes::of(&ethernet), StepHashes::of(&strict));

    assert_eq!(before.access_point, after.access_point, "the AP is untouched by a mode change — no client bounces");
    assert_eq!(before.ap_activation, after.ap_activation, "…and neither is its activation");
    assert_ne!(before.mode, after.mode, "the mode step must re-run");
    assert_ne!(before.wwan, after.wwan, "…and so must the bearer: the mode drives its metric and autoconnect");
    assert_ne!(before.uplink_env, after.uplink_env, "the supervisor is told the new mode");
}

/// Each step's hash covers exactly its own inputs, in both directions.
#[test]
fn each_step_keys_on_its_own_inputs() {
    let base = populated();

    // An SSID change is the AP's business and nothing else's.
    let mut renamed = populated();
    renamed.ap.ssid = "ContextPilot-beef".to_owned();
    let after = StepHashes::of(&renamed);
    assert_ne!(StepHashes::of(&base).access_point, after.access_point, "the profile must be rewritten");
    assert_eq!(StepHashes::of(&base).ap_activation, after.ap_activation, "but the AP is not bounced");
    assert_eq!(StepHashes::of(&base).wwan, after.wwan, "and the bearer is not touched at all — the other half");
    assert_eq!(StepHashes::of(&base).mode, after.mode);

    // Probe tuning is the supervisor's business and nothing else's.
    let mut retuned = populated();
    retuned.probe.cooldown_s = 15;
    let tuned = StepHashes::of(&retuned);
    assert_ne!(StepHashes::of(&base).uplink_env, tuned.uplink_env, "the env file is re-rendered");
    assert_eq!(StepHashes::of(&base).access_point, tuned.access_point, "…and nothing on the radio moves");
    assert_eq!(StepHashes::of(&base).mode, tuned.mode);

    // A standby switch is what `apply_mode` acts on, so it must re-run: `hot`
    // keeps the bearer up, `cold` takes it down.
    let mut cold = populated();
    cold.wwan.standby = Standby::Cold;
    assert_ne!(StepHashes::of(&base).mode, StepHashes::of(&cold).mode, "hot → cold must bring the bearer down");

    // A secret change with every other field identical still reconciles.
    let mut rekeyed = populated();
    rekeyed.ap.passphrase = Some("a-completely-different-psk".to_owned());
    assert_ne!(StepHashes::of(&base).access_point, StepHashes::of(&rekeyed).access_point);
}

/// The load-bearing one. This is `commit`'s rollback, played out against
/// the marks: `apply(next)` gets partway and fails, then `apply(previous)` must
/// **re-run every step that actually ran**.
///
/// The old whole-document marker made this impossible: it was written only after
/// a complete apply, so it still held `fingerprint(previous)`, the rollback's
/// `apply(previous)` matched it and performed no system work at all.
#[test]
fn a_partial_apply_leaves_the_rollback_with_work_to_do() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("cp-network-applied");

    let previous = populated();
    let mut next = populated();
    next.mode = UplinkMode::FiveG;
    next.ap.ssid = "ContextPilot-next".to_owned();

    // A first, complete apply of `previous` — every step runs and is recorded.
    let mut marks = Marks::load(&marker);
    let before = StepHashes::of(&previous);
    let mut ran = Vec::new();
    for (name, hash) in
        [(STEP_WWAN, before.wwan.clone()), (STEP_AP, before.access_point.clone()), (STEP_MODE, before.mode.clone())]
    {
        step(&mut marks, name, hash, || {
            ran.push(name);
            Ok(())
        })
        .expect("the first apply succeeds");
    }
    assert_eq!(ran, [STEP_WWAN, STEP_AP, STEP_MODE], "a fresh boot reconciles everything for real");

    // Now `apply(next)`: the bearer and the AP succeed, the mode step fails.
    let mut marks = Marks::load(&marker);
    let after = StepHashes::of(&next);
    step(&mut marks, STEP_WWAN, after.wwan, || Ok(())).expect("bearer ok");
    step(&mut marks, STEP_AP, after.access_point, || Ok(())).expect("ap ok");
    let failed = step(&mut marks, STEP_MODE, after.mode, || Err("networkctl reconfigure failed".to_owned()));
    assert!(failed.is_err(), "the mode step failed partway through the apply");

    // The rollback. Every step that RAN with `next` must run again with
    // `previous`; the one that never ran is correctly skipped.
    let mut marks = Marks::load(&marker);
    let mut rolled_back = Vec::new();
    for (name, hash) in
        [(STEP_WWAN, before.wwan.clone()), (STEP_AP, before.access_point.clone()), (STEP_MODE, before.mode)]
    {
        step(&mut marks, name, hash, || {
            rolled_back.push(name);
            Ok(())
        })
        .expect("the rollback succeeds");
    }
    assert_eq!(
        rolled_back,
        [STEP_WWAN, STEP_AP],
        "the two steps that ran are undone; the mode step never ran, so it has nothing to undo"
    );
}

/// A step is skipped only when its own inputs are unchanged, and a mark is
/// written the moment its step succeeds — not at the end of the apply.
#[test]
fn a_mark_is_recorded_the_instant_its_step_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("cp-network-applied");
    let hashes = StepHashes::of(&populated());

    let mut marks = Marks::load(&marker);
    assert!(!marks.unchanged(STEP_AP, &hashes.access_point), "nothing has ever run on a fresh boot");
    step(&mut marks, STEP_AP, hashes.access_point.clone(), || Ok(())).expect("ap ok");

    // On disk already, before any later step has been attempted.
    let body = std::fs::read_to_string(&marker).expect("the marker exists");
    assert!(body.contains(&format!("{STEP_AP}={}\n", hashes.access_point)), "flushed immediately: {body}");
    assert!(!body.contains(STEP_UPLINK_ENV), "and only for the step that ran: {body}");

    // A second apply of the same document does no work…
    let mut reloaded = Marks::load(&marker);
    let mut ran = false;
    step(&mut reloaded, STEP_AP, hashes.access_point, || {
        ran = true;
        Ok(())
    })
    .expect("second ap");
    assert!(!ran, "an unchanged step is skipped — that is what stops the Wi-Fi bouncing");

    // …and a step that has never run is not skipped by another step's mark.
    assert!(!reloaded.unchanged(STEP_AP_ACTIVATION, &hashes.ap_activation));
}

/// A failed step must NOT be recorded — otherwise the next apply would skip the
/// very work that did not happen.
#[test]
fn a_failed_step_records_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("cp-network-applied");
    let hashes = StepHashes::of(&populated());

    let mut marks = Marks::load(&marker);
    let outcome = step(&mut marks, STEP_WWAN, hashes.wwan.clone(), || Err("nmcli: no such device".to_owned()));
    assert_eq!(outcome, Err("nmcli: no such device".to_owned()), "the failure reaches the caller untouched");
    assert!(!Marks::load(&marker).unchanged(STEP_WWAN, &hashes.wwan), "the step still has work to do");
}

// ── No modem ⇒ no 5G mode, below the transport layer ────────────────────────

/// The "no modem ⇒ no 5G mode" guard lived only in the HTTP handlers, so a
/// document seeded `5g` by `-e net_mode=5g` on a non-5G variant suppressed
/// `end0`'s default route at **every boot**, with no way back from the cockpit.
#[test]
fn a_modem_less_box_applies_wan_whatever_the_document_says() {
    for wanted in [UplinkMode::FiveG, UplinkMode::WanThen5g] {
        let mut document = populated();
        document.mode = wanted;
        let effective = coerce_mode(&document, false);
        assert_eq!(effective.mode, UplinkMode::Wan, "{wanted:?} is coerced on a box with no modem");
        // Refusing the whole apply would also take the AP down. "Route over
        // ethernet, keep everything else" is the honest posture.
        assert_eq!(effective.ap, document.ap, "the access point is untouched");
        assert_eq!(effective.wwan, document.wwan, "and so is the bearer configuration");
        assert_eq!(document.mode, wanted, "the persisted document keeps the admin's stated intent");
    }
}

/// …and on a box that *has* a modem, nothing is coerced at all.
#[test]
fn a_5g_variant_applies_exactly_what_it_was_given() {
    for wanted in [UplinkMode::Wan, UplinkMode::WanThen5g, UplinkMode::FiveG] {
        let mut document = populated();
        document.mode = wanted;
        assert_eq!(coerce_mode(&document, true).mode, wanted);
    }
    // `wan` needs no modem, so it is never coerced even without one.
    let mut ethernet = populated();
    ethernet.mode = UplinkMode::Wan;
    assert_eq!(coerce_mode(&ethernet, false).mode, UplinkMode::Wan);
}

// ── The three supervisor knobs ──────────────────────────────────────────────

/// `probe_timeout_s`, `cooldown_s` and `nm_wait_s` once had no field, so they
/// were permanently 3/60/20 and unreachable. Deliberately **no**
/// `#[serde(default)]`: a template/struct mismatch must be loud — `load` names
/// what the box just lost — rather than a field that silently reverts to a
/// default nobody chose.
#[test]
fn the_three_supervisor_knobs_are_persisted_and_bounded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = state::network_path(dir.path());
    let mut config = populated();
    config.probe.probe_timeout_s = 5;
    config.probe.cooldown_s = 300;
    config.probe.nm_wait_s = 90;
    state::save(&path, &config).expect("save");
    assert_eq!(state::load(&path).probe, config.probe, "all three round-trip");

    let defaults = ProbeConfig::default();
    assert_eq!((defaults.probe_timeout_s, defaults.cooldown_s, defaults.nm_wait_s), (3, 60, 20), "the shell's values");

    // …and an out-of-range value costs the whole document, fail-closed, the same
    // way every other invalid field does. The bounds themselves are exercised
    // directly against `uplink::validate_probe`.
    let mut bad = populated();
    bad.probe.nm_wait_s = 121;
    std::fs::write(&path, serde_json::to_vec(&bad).expect("serialize")).expect("write");
    assert_eq!(state::load(&path), NetworkConfig::default(), "an unhonourable nmcli wait is refused");
}

/// A document written by an older template — one missing the three new fields —
/// must be **rejected loudly**, not silently half-loaded. That is the whole
/// reason this struct carries no `#[serde(default)]` anywhere.
#[test]
fn a_document_missing_the_new_probe_fields_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = state::network_path(dir.path());
    let old = serde_json::json!({
        "mode": "wan_5g",
        "wwan": { "apn": "orange.fr", "username": null, "password": null, "pin": null,
                  "roaming": false, "standby": "hot" },
        "ap": { "enabled": false, "ssid": "ContextPilot", "passphrase": null, "band": "a",
                "channel": 0, "country": "FR", "hidden": false, "share_internet": true },
        "probe": { "targets": ["1.1.1.1"], "fail_threshold": 3, "ok_threshold": 2, "interval_s": 10 },
    });
    std::fs::write(&path, serde_json::to_vec(&old).expect("serialize")).expect("write");
    assert_eq!(state::load(&path), NetworkConfig::default(), "a stale template shape is fail-closed, and logged");
}

/// The read projection carries them too — a cockpit that cannot see a value
/// cannot offer to change it.
#[test]
fn the_read_projection_exposes_the_probe_tuning() {
    let projected = populated().redacted(true);
    let probe = projected.get("probe").expect("the probe block is projected");
    for field in
        ["targets", "fail_threshold", "ok_threshold", "interval_s", "probe_timeout_s", "cooldown_s", "nm_wait_s"]
    {
        assert!(probe.get(field).is_some(), "{field} is missing from the read path");
    }
}
