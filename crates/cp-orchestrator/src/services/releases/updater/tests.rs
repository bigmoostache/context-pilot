//! Updater unit tests — signature/freshness/anti-rollback (V3.1), verified
//! download (V3.2), and the atomic apply + rollback cycle (V3.3), all on
//! local fixtures. The manifest fixtures are signed with the **real** release
//! key (regenerating them requires the signing host — see update-policy
//! §5.4.1), so these tests exercise the exact embedded trust anchor.

use super::super::{ReleaseStore, boot_check, boot_commit_when_healthy, semver_sort_key};
use super::apply::{boot_reconcile, promote_committed, stage_apply};
use super::download::{tag_dir, verify_and_extract};
use super::state::{UpdateResult, UpdateState};
use super::verify::iso8601_to_epoch;
use super::{UpdateEvaluation, check_and_prepare, evaluate_manifest};

/// CI-shaped manifest (version `v9.9.9`, `min_from v0.2.0`, expires 2126),
/// signed with the release key.
const VALID_JSON: &[u8] = include_bytes!("fixtures/stable-valid.json");
const VALID_SIG: &str = include_str!("fixtures/stable-valid.json.minisig");
/// Same shape but `expires_at` 2020 — a stale (yet correctly signed) replay.
const EXPIRED_JSON: &[u8] = include_bytes!("fixtures/stable-expired.json");
const EXPIRED_SIG: &str = include_str!("fixtures/stable-expired.json.minisig");

/// A fixed "now" (2027-01-15) — after the fixtures' release, before expiry.
const NOW: u64 = 1_800_000_000;

/// Fresh temp dir for a test, cleaned by the caller.
fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cp-updater-{label}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

// ── O3.1 — manifest verification ────────────────────────────────────────────

/// V3.1a — the verification matrix: valid signature accepted; tampered bytes
/// rejected; tampered signature rejected; expired manifest rejected; offered
/// version at/below current rejected (`UpToDate` at equality); a box below
/// `min_from` refused.
#[test]
fn updater_verify() {
    // Valid manifest, older box → Available(v9.9.9).
    match evaluate_manifest(VALID_JSON, VALID_SIG, "v0.3.0", NOW, "stable", false) {
        Ok(UpdateEvaluation::Available(m)) => assert_eq!(m.version, "v9.9.9"),
        other => panic!("valid manifest must be Available: {other:?}"),
    }

    // One flipped byte in the signed JSON → signature failure.
    let mut tampered = VALID_JSON.to_vec();
    let pos = tampered.iter().position(|&b| b == b'9').expect("a '9' in the fixture");
    tampered[pos] = b'8';
    assert!(
        matches!(
            evaluate_manifest(&tampered, VALID_SIG, "v0.3.0", NOW, "stable", false),
            Err(super::VerifyError::Signature(_))
        ),
        "tampered manifest bytes must fail the signature check"
    );

    // A corrupted signature blob → signature failure.
    let bad_sig = VALID_SIG.replace('A', "B");
    assert!(
        matches!(
            evaluate_manifest(VALID_JSON, &bad_sig, "v0.3.0", NOW, "stable", false),
            Err(super::VerifyError::Signature(_))
        ),
        "tampered signature must fail"
    );

    // Correctly signed but expired → freshness rejection (stale replay).
    assert!(
        matches!(
            evaluate_manifest(EXPIRED_JSON, EXPIRED_SIG, "v0.3.0", NOW, "stable", false),
            Err(super::VerifyError::Expired { .. })
        ),
        "expired manifest must be rejected"
    );

    // Anti-rollback: offered (v9.9.9) below the running version.
    assert!(
        matches!(
            evaluate_manifest(VALID_JSON, VALID_SIG, "v10.0.0", NOW, "stable", false),
            Err(super::VerifyError::Rollback { .. })
        ),
        "manifest older than the running version must be rejected"
    );

    // min_from floor: a box on v0.1.0 may not jump (min_from is v0.2.0).
    assert!(
        matches!(
            evaluate_manifest(VALID_JSON, VALID_SIG, "v0.1.0", NOW, "stable", false),
            Err(super::VerifyError::TooOldForJump { .. })
        ),
        "a box below min_from must be refused"
    );

    // Same version → UpToDate (not an error, not an update).
    assert!(
        matches!(
            evaluate_manifest(VALID_JSON, VALID_SIG, "v9.9.9", NOW, "stable", false),
            Ok(UpdateEvaluation::UpToDate)
        ),
        "equal version is up to date"
    );
}

/// Bare manifest for the pure-decision tests — [`evaluate_parsed`] ignores
/// artifacts, so only channel/version/expires/min_from need to be real.
fn decision_manifest(channel: &str, version: &str, expires_at: &str, min_from: &str) -> super::super::Manifest {
    super::super::Manifest {
        schema: 1,
        channel: channel.to_owned(),
        version: version.to_owned(),
        released_at: "2026-07-10T12:00:00Z".to_owned(),
        expires_at: expires_at.to_owned(),
        min_from: min_from.to_owned(),
        notes_url: String::new(),
        artifacts: std::collections::BTreeMap::new(),
    }
}

/// The channel-aware "is-newer" matrix (nightly head-tracking, the
/// channel-equality guard, and forced adoption on a switch), exercised on
/// already-parsed manifests — the fixtures are signed with the real release key
/// so a nightly one cannot be minted, and this logic sits after the signature.
#[test]
fn updater_channel_decisions() {
    use super::VerifyError;
    use super::verify::evaluate_parsed;

    /// Fresh well past `NOW` (2027-01-15).
    const FRESH: &str = "2126-01-01T00:00:00Z";

    // Nightly head-tracking: a different sha at the same (M,m,p) is an update…
    let m = decision_manifest("nightly", "v0.1.0-def5678", FRESH, "v0.1.0");
    assert!(
        matches!(evaluate_parsed(m, "v0.1.0-abc1234", NOW, "nightly", false), Ok(UpdateEvaluation::Available(_))),
        "a new nightly sha at the same semver must be Available"
    );
    // …and the identical version is UpToDate (no perpetual re-apply).
    let m = decision_manifest("nightly", "v0.1.0-abc1234", FRESH, "v0.1.0");
    assert!(
        matches!(evaluate_parsed(m, "v0.1.0-abc1234", NOW, "nightly", false), Ok(UpdateEvaluation::UpToDate)),
        "the same nightly build is up to date"
    );

    // Nightly ignores the min_from floor (head-tracking drops anti-rollback).
    let m = decision_manifest("nightly", "v0.1.0-def5678", FRESH, "v9.9.9");
    assert!(
        matches!(evaluate_parsed(m, "v0.0.1-old", NOW, "nightly", false), Ok(UpdateEvaluation::Available(_))),
        "nightly skips the min_from floor"
    );

    // Freshness still bites on nightly: an expired manifest is a stale replay.
    let m = decision_manifest("nightly", "v0.1.0-def5678", "2020-01-01T00:00:00Z", "v0.1.0");
    assert!(
        matches!(evaluate_parsed(m, "v0.1.0-abc1234", NOW, "nightly", false), Err(VerifyError::Expired { .. })),
        "an expired nightly manifest is refused"
    );

    // Channel-equality guard: a signed *stable* manifest served where nightly
    // is expected is refused — the only guard left on the head-tracking path.
    let m = decision_manifest("stable", "v0.2.12", FRESH, "v0.1.0");
    assert!(
        matches!(evaluate_parsed(m, "v0.1.0-abc1234", NOW, "nightly", false), Err(VerifyError::ChannelMismatch { .. })),
        "a cross-channel manifest must be refused"
    );

    // Forced adoption on an explicit switch: crossgrade bypasses the semver
    // anti-rollback that would otherwise reject a lower stable version.
    let m = decision_manifest("stable", "v0.1.0", FRESH, "v0.1.0");
    assert!(
        matches!(evaluate_parsed(m, "v0.2.0", NOW, "stable", false), Err(VerifyError::Rollback { .. })),
        "without a switch a lower version is a rollback"
    );
    let m = decision_manifest("stable", "v0.1.0", FRESH, "v0.1.0");
    assert!(
        matches!(evaluate_parsed(m, "v0.2.0", NOW, "stable", true), Ok(UpdateEvaluation::Available(_))),
        "an explicit switch adopts the target head regardless of version order"
    );
    // A crossgrade to the identical version is still a no-op.
    let m = decision_manifest("stable", "v0.2.0", FRESH, "v0.1.0");
    assert!(
        matches!(evaluate_parsed(m, "v0.2.0", NOW, "stable", true), Ok(UpdateEvaluation::UpToDate)),
        "crossgrade to the same version is up to date"
    );
}

/// V3.1b — the download hook is provably never invoked when any verification
/// fails; it runs exactly once on a verified available update.
#[test]
fn updater_no_download_on_failed_check() {
    let mut tampered = VALID_JSON.to_vec();
    tampered[0] = b' ';

    for (bytes, sig, current) in [
        (tampered.as_slice(), VALID_SIG, "v0.3.0"), // bad signature
        (EXPIRED_JSON, EXPIRED_SIG, "v0.3.0"),      // expired
        (VALID_JSON, VALID_SIG, "v10.0.0"),         // rollback
        (VALID_JSON, VALID_SIG, "v0.1.0"),          // below min_from
    ] {
        let mut downloaded = false;
        let outcome = check_and_prepare(bytes, sig, current, NOW, "stable", false, |_m| {
            downloaded = true;
            Ok(())
        });
        assert!(outcome.is_err(), "failed check must surface as Err");
        assert!(!downloaded, "download must never run on a failed check");
    }

    // Up to date: no error, no download.
    let mut downloaded = false;
    let outcome = check_and_prepare(VALID_JSON, VALID_SIG, "v9.9.9", NOW, "stable", false, |_m| {
        downloaded = true;
        Ok(())
    });
    assert!(matches!(outcome, Ok(None)), "up to date");
    assert!(!downloaded);

    // Available: download runs, manifest is returned.
    let mut downloaded = false;
    let outcome = check_and_prepare(VALID_JSON, VALID_SIG, "v0.3.0", NOW, "stable", false, |m| {
        assert_eq!(m.version, "v9.9.9");
        downloaded = true;
        Ok(())
    });
    assert!(matches!(outcome, Ok(Some(_))), "verified update prepared");
    assert!(downloaded, "download must run for a verified available update");
}

/// The one timestamp shape CI emits parses; garbage does not; ordering holds.
#[test]
fn updater_timestamp_parsing() {
    let t0 = iso8601_to_epoch("1970-01-01T00:00:00Z").expect("epoch start");
    assert_eq!(t0, 0);
    let t1 = iso8601_to_epoch("2026-07-10T12:00:00Z").expect("valid");
    let t2 = iso8601_to_epoch("2126-01-01T00:00:00Z").expect("valid");
    assert!(t0 < t1 && t1 < t2, "chronological order maps to numeric order");
    for garbage in ["2026-07-10", "not a date", "2026-13-01T00:00:00Z", "2026-07-10T25:00:00Z", ""] {
        assert!(iso8601_to_epoch(garbage).is_none(), "{garbage:?} must be rejected");
    }
}

// ── O3.2 — sha256-verified download ─────────────────────────────────────────

/// Build a real `.tar.gz` (via the system tar, like extraction) holding the
/// two release binaries; returns its bytes.
fn make_release_tarball(dir: &std::path::Path, cpilot: &[u8], orchestrator: &[u8]) -> Vec<u8> {
    let staging = dir.join("staging");
    std::fs::create_dir_all(&staging).expect("staging dir");
    std::fs::write(staging.join("cpilot"), cpilot).expect("write cpilot");
    std::fs::write(staging.join("cp-orchestrator"), orchestrator).expect("write orchestrator");
    let tarball = dir.join("bundle.tar.gz");
    let status = std::process::Command::new("tar")
        .args(["czf", &tarball.to_string_lossy(), "-C", &staging.to_string_lossy(), "."])
        .status()
        .expect("tar available");
    assert!(status.success(), "tarball built");
    std::fs::read(&tarball).expect("read tarball")
}

/// Lower-hex SHA-256, test-side (independent of the implementation's helper).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes).iter().fold(String::with_capacity(64), |mut acc, b| {
        use std::fmt::Write as _;
        let _w = write!(acc, "{b:02x}");
        acc
    })
}

/// V3.2a — a tarball whose sha256 differs from the manifest pin → `Err`, the
/// tag directory does not exist, and the store's selection is untouched.
#[test]
fn updater_download_rejects_sha_mismatch() {
    let dir = temp_dir("sha-ko");
    let store = ReleaseStore::load(dir.join("releases"));
    let bytes = make_release_tarball(&dir, b"CPILOT", b"ORCH");

    let wrong = "0".repeat(64);
    let outcome = verify_and_extract(&store, "v1.2.3", &bytes, &wrong);
    assert!(outcome.is_err(), "mismatching sha must abort");
    assert!(!tag_dir(&store, "v1.2.3").exists(), "nothing extracted, directory cleaned");
    assert!(store.active_tag().is_none(), "selection untouched");

    drop(std::fs::remove_dir_all(&dir));
}

/// V3.2b — a matching sha256 extracts both binaries into `releases/<tag>/`.
#[test]
fn updater_download_extracts_on_sha_match() {
    let dir = temp_dir("sha-ok");
    let store = ReleaseStore::load(dir.join("releases"));
    let bytes = make_release_tarball(&dir, b"CPILOT-BYTES", b"ORCH-BYTES");

    verify_and_extract(&store, "v1.2.3", &bytes, &sha256_hex(&bytes)).expect("verified extraction");
    assert_eq!(std::fs::read(store.binary_path("v1.2.3")).expect("cpilot"), b"CPILOT-BYTES");
    assert_eq!(std::fs::read(store.orchestrator_binary_path("v1.2.3")).expect("orch"), b"ORCH-BYTES");

    drop(std::fs::remove_dir_all(&dir));
}

// ── O3.3 — atomic apply, promote, rollback ──────────────────────────────────

/// One staged-apply fixture: releases vX (active) + vY on disk, an install
/// binary, an auth.db. Returns (base, store, install, auth_db).
fn apply_fixture(label: &str) -> (std::path::PathBuf, ReleaseStore, std::path::PathBuf, std::path::PathBuf) {
    let base = temp_dir(label);
    let releases = base.join("releases");
    for (tag, orch, cpilot) in [("vX", "ORCH-VX", "CPILOT-VX"), ("vY", "ORCH-VY", "CPILOT-VY")] {
        let d = releases.join(tag);
        std::fs::create_dir_all(&d).expect("tag dir");
        std::fs::write(d.join("cp-orchestrator"), orch).expect("orch");
        std::fs::write(d.join("cpilot"), cpilot).expect("cpilot");
    }
    let mut store = ReleaseStore::load(releases);
    let _bin = store.select("vX").expect("select vX");

    let install = base.join("bin").join("cp-orchestrator");
    std::fs::create_dir_all(base.join("bin")).expect("bin dir");
    std::fs::write(&install, "ORCH-VX").expect("install binary");

    let auth_db = base.join("auth.db");
    std::fs::write(&auth_db, "DB-V1").expect("auth db");
    (base, store, install, auth_db)
}

/// V3.3a + V3.3c — healthy vX→vY: stage swaps the orchestrator and backs up
/// the DB; the health-gated commit + promote flips `active_tag` and the agent
/// binary to vY, cleans every marker, and drops the DB backup. Both binaries
/// end on the same tag.
#[test]
fn updater_apply_healthy_cycle() {
    let (base, mut store, install, auth_db) = apply_fixture("apply-ok");

    stage_apply(&store, None, &auth_db, &install, "vY").expect("stage");
    // The install path already holds vY bytes; rollback material is in place.
    assert_eq!(std::fs::read(&install).expect("install"), b"ORCH-VY");
    assert!(install.with_file_name("cp-orchestrator.pending").exists(), ".pending marker");
    assert!(install.with_file_name("cp-orchestrator.bak").exists(), ".bak backup");
    let backup = auth_db.with_file_name("auth.db.bak-vX");
    assert_eq!(std::fs::read(&backup).expect("db backup"), b"DB-V1");
    assert_eq!(store.active_tag(), Some("vX"), "active_tag flips only on commit");

    // Healthy boot: binary markers commit, then the release state promotes.
    assert!(boot_commit_when_healthy(
        &install,
        || true,
        std::time::Duration::from_secs(1),
        std::time::Duration::from_millis(1)
    ));
    let agent_binary = promote_committed(&mut store, &auth_db).expect("promote").expect("an update was in flight");

    // V3.3a — new tag active, markers + backup gone.
    assert_eq!(store.active_tag(), Some("vY"));
    assert!(!install.with_file_name("cp-orchestrator.pending").exists());
    assert!(!install.with_file_name("cp-orchestrator.bak").exists());
    assert!(!backup.exists(), "db backup removed on commit");
    assert!(!store.dir().join("pending-update.json").exists());
    // V3.3c — both binaries point at the SAME tag.
    assert_eq!(agent_binary, store.binary_path("vY"), "agent binary repointed to vY");
    assert_eq!(std::fs::read(&agent_binary).expect("cpilot"), b"CPILOT-VY");
    assert_eq!(std::fs::read(&install).expect("install"), b"ORCH-VY");
    // Status recorded.
    let st = UpdateState::load(store.dir());
    assert!(
        matches!(st.last_result, Some(UpdateResult::Success { ref to, .. }) if to == "vY"),
        "success recorded: {st:?}"
    );

    drop(std::fs::remove_dir_all(&base));
}

/// V3.3b — a crash-looping vY: after `MAX_BOOT_ATTEMPTS` the binary guard
/// restores vX, and boot reconciliation restores `auth.db` from the backup
/// (a forward migration ran in between), keeps `active_tag` at vX, and
/// records `rolled_back`.
#[test]
fn updater_apply_rollback_cycle() {
    let (base, store, install, auth_db) = apply_fixture("apply-ko");

    stage_apply(&store, None, &auth_db, &install, "vY").expect("stage");
    // vY runs long enough to migrate the database, then keeps crashing.
    std::fs::write(&auth_db, "DB-V2-MIGRATED").expect("simulate forward migration");
    boot_check(&install); // attempt 1 — still within tolerance
    boot_check(&install); // attempt 2 — rolls the binary back
    assert_eq!(std::fs::read(&install).expect("install"), b"ORCH-VX", "binary rolled back");
    assert!(!install.with_file_name("cp-orchestrator.pending").exists(), "binary markers cleared");

    // The old binary boots and reconciles the interrupted update.
    boot_reconcile(store.dir(), &auth_db, &install);
    assert_eq!(std::fs::read(&auth_db).expect("auth db"), b"DB-V1", "database restored from backup");
    assert!(!auth_db.with_file_name("auth.db.bak-vX").exists(), "backup consumed");
    assert!(!store.dir().join("pending-update.json").exists(), "in-flight record cleared");

    let reloaded = ReleaseStore::load(store.dir().to_path_buf());
    assert_eq!(reloaded.active_tag(), Some("vX"), "active_tag never flipped");
    let st = UpdateState::load(store.dir());
    assert!(
        matches!(st.last_result, Some(UpdateResult::RolledBack { ref attempted, .. }) if attempted == "vY"),
        "rollback recorded: {st:?}"
    );

    drop(std::fs::remove_dir_all(&base));
}

// ── O4.2 — scheduler tick decisions ─────────────────────────────────────────

use std::sync::atomic::{AtomicBool, Ordering};

use super::super::{MaintenanceWindow, UpdateMode};
use super::scheduler::{TickOutcome, run_tick};

/// The valid fixture parsed into a [`Manifest`](super::super::Manifest).
fn fixture_manifest() -> super::super::Manifest {
    serde_json::from_slice(VALID_JSON).expect("fixture parses")
}

/// Drive one tick with an injected clock + hooks; returns the outcome and
/// whether each hook ran.
fn tick(mode: UpdateMode, now_minutes: u16, gate: &AtomicBool, available: bool) -> (TickOutcome, bool, bool) {
    let window = MaintenanceWindow::default(); // 03:00–05:00
    let mut checked = false;
    let mut applied = false;
    let outcome = run_tick(
        mode,
        &window,
        now_minutes,
        gate,
        || {
            checked = true;
            Ok(if available { Some(fixture_manifest()) } else { None })
        },
        |m| {
            assert_eq!(m.version, "v9.9.9");
            applied = true;
            Ok("v0.3.0".to_owned())
        },
    );
    (outcome, checked, applied)
}

/// In-window (03:30) and out-of-window (12:00) instants for the default window.
const IN_WINDOW: u16 = 3 * 60 + 30;
const OUT_WINDOW: u16 = 12 * 60;

/// V4.2a — the decision matrix with an injected clock: `auto` + in-window +
/// available → the pipeline runs; out-of-window → it does not; `manual` does
/// not even in-window; `paused` never. Every mode still checks.
#[test]
fn scheduler_tick_decisions() {
    // auto + in-window + available → apply.
    let gate = AtomicBool::new(false);
    let (outcome, checked, applied) = tick(UpdateMode::Auto, IN_WINDOW, &gate, true);
    assert!(matches!(outcome, TickOutcome::Applied { ref to, .. } if to == "v9.9.9"), "{outcome:?}");
    assert!(checked && applied);

    // auto + out-of-window → no apply.
    let gate = AtomicBool::new(false);
    let (outcome, checked, applied) = tick(UpdateMode::Auto, OUT_WINDOW, &gate, true);
    assert!(matches!(outcome, TickOutcome::SkipWindow { .. }), "{outcome:?}");
    assert!(checked && !applied, "out-of-window must not apply");

    // manual + in-window → no apply (but the check refreshed state).
    let gate = AtomicBool::new(false);
    let (outcome, checked, applied) = tick(UpdateMode::Manual, IN_WINDOW, &gate, true);
    assert!(matches!(outcome, TickOutcome::SkipMode(UpdateMode::Manual)), "{outcome:?}");
    assert!(checked && !applied, "manual never auto-applies");

    // paused → never, in or out of the window.
    for now in [IN_WINDOW, OUT_WINDOW] {
        let gate = AtomicBool::new(false);
        let (outcome, checked, applied) = tick(UpdateMode::Paused, now, &gate, true);
        assert!(matches!(outcome, TickOutcome::SkipMode(UpdateMode::Paused)), "{outcome:?}");
        assert!(checked && !applied, "paused never applies");
    }

    // Up to date → nothing to do in any mode.
    let gate = AtomicBool::new(false);
    let (outcome, checked, applied) = tick(UpdateMode::Auto, IN_WINDOW, &gate, false);
    assert!(matches!(outcome, TickOutcome::UpToDate), "{outcome:?}");
    assert!(checked && !applied);

    // A failed check applies nothing.
    let gate = AtomicBool::new(false);
    let mut applied = false;
    let outcome = run_tick(
        UpdateMode::Auto,
        &MaintenanceWindow::default(),
        IN_WINDOW,
        &gate,
        || Err("boom".to_owned()),
        |_m| {
            applied = true;
            Ok(String::new())
        },
    );
    assert!(matches!(outcome, TickOutcome::CheckFailed(_)), "{outcome:?}");
    assert!(!applied, "no apply on a failed check");
}

/// V4.2b — applies are serialised: once one is in flight the gate refuses a
/// second, and only a *failed* apply releases the gate for a retry.
#[test]
fn scheduler_serialises_applies() {
    let gate = AtomicBool::new(false);

    // First tick applies and keeps the gate held (a restart is pending).
    let (outcome, _c, applied) = tick(UpdateMode::Auto, IN_WINDOW, &gate, true);
    assert!(matches!(outcome, TickOutcome::Applied { .. }), "{outcome:?}");
    assert!(applied && gate.load(Ordering::SeqCst), "gate held after a successful apply");

    // A second close tick must not launch a concurrent apply.
    let (outcome, checked, applied) = tick(UpdateMode::Auto, IN_WINDOW, &gate, true);
    assert!(matches!(outcome, TickOutcome::SkipInFlight), "{outcome:?}");
    assert!(checked && !applied, "no concurrent apply");

    // A failed apply releases the gate so the next tick can retry.
    let gate = AtomicBool::new(false);
    let outcome = run_tick(
        UpdateMode::Auto,
        &MaintenanceWindow::default(),
        IN_WINDOW,
        &gate,
        || Ok(Some(fixture_manifest())),
        |_m| Err("download broke".to_owned()),
    );
    assert!(matches!(outcome, TickOutcome::ApplyFailed(_)), "{outcome:?}");
    assert!(!gate.load(Ordering::SeqCst), "failed apply releases the gate");
    let (outcome, _c, applied) = tick(UpdateMode::Auto, IN_WINDOW, &gate, true);
    assert!(matches!(outcome, TickOutcome::Applied { .. }), "{outcome:?}");
    assert!(applied, "retry allowed after a failure");
}

/// Sanity: the fixtures' version ordering matches the comparator the
/// anti-rollback check uses.
#[test]
fn updater_fixture_ordering() {
    assert!(semver_sort_key("v9.9.9") > semver_sort_key("v0.3.0"));
    assert!(semver_sort_key("v9.9.9") < semver_sort_key("v10.0.0"));
}
