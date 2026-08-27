//! O4.2 — scheduler tick decisions. Split from `tests/mod.rs` (the flat file
//! breached the 500-line cap); the shared fixtures/consts live in the parent
//! and reach here via `use super::*`.

use std::sync::atomic::{AtomicBool, Ordering};

use super::super::super::{MaintenanceWindow, UpdateMode};
use super::super::scheduler::{TickCtx, TickOutcome, run_tick};
use super::*;

/// The valid fixture parsed into a [`Manifest`](super::super::super::Manifest).
fn fixture_manifest() -> super::super::super::Manifest {
    serde_json::from_slice(VALID_JSON).expect("fixture parses")
}

/// Drive one tick with an injected clock + hooks; returns the outcome and
/// whether each hook ran.
fn tick(mode: UpdateMode, now_minutes: u16, gate: &AtomicBool, available: bool) -> (TickOutcome, bool, bool) {
    let window = MaintenanceWindow::default(); // 03:00–05:00
    let mut checked = false;
    let mut applied = false;
    let outcome = run_tick(
        &TickCtx { mode, window: &window, now_minutes, apply_gate: gate },
        || {
            checked = true;
            Ok(available.then(fixture_manifest))
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

/// Run one tick on a fresh gate and assert the outcome matches `want` and the
/// hook flags. Each call owns its scope so sequential cases don't shadow; the
/// `applied` flag is expected exactly when `want` is [`TickOutcome::Applied`].
fn expect_tick(mode: UpdateMode, now: u16, available: bool, want: &TickOutcome) {
    let gate = AtomicBool::new(false);
    let (outcome, checked, applied) = tick(mode, now, &gate, available);
    assert_eq!(&outcome, want, "outcome");
    assert!(checked, "every mode checks");
    assert_eq!(applied, matches!(want, TickOutcome::Applied { .. }), "applied flag");
}

/// V4.2a — the decision matrix with an injected clock: `auto` + in-window +
/// available → the pipeline runs; out-of-window → it does not; `manual` does
/// not even in-window; `paused` never. Every mode still checks.
#[test]
fn scheduler_tick_decisions() {
    let applied = TickOutcome::Applied { from: "v0.3.0".to_owned(), to: "v9.9.9".to_owned() };
    expect_tick(UpdateMode::Auto, IN_WINDOW, true, &applied);
    expect_tick(UpdateMode::Auto, OUT_WINDOW, true, &TickOutcome::SkipWindow { available: "v9.9.9".to_owned() });
    expect_tick(UpdateMode::Manual, IN_WINDOW, true, &TickOutcome::SkipMode(UpdateMode::Manual));
    expect_tick(UpdateMode::Paused, IN_WINDOW, true, &TickOutcome::SkipMode(UpdateMode::Paused));
    expect_tick(UpdateMode::Paused, OUT_WINDOW, true, &TickOutcome::SkipMode(UpdateMode::Paused));
    expect_tick(UpdateMode::Auto, IN_WINDOW, false, &TickOutcome::UpToDate);
    assert_check_failed();
}

/// A failed channel check applies nothing (kept out of the main body so the
/// decision test stays a flat list of `expect_tick` calls).
fn assert_check_failed() {
    let gate = AtomicBool::new(false);
    let outcome = run_tick(
        &TickCtx { mode: UpdateMode::Auto, window: &MaintenanceWindow::default(), now_minutes: IN_WINDOW, apply_gate: &gate },
        || Err("boom".to_owned()),
        |_m| Ok(String::new()),
    );
    assert!(matches!(outcome, TickOutcome::CheckFailed(_)), "{outcome:?}");
}

/// V4.2b — applies are serialised: once one is in flight the gate refuses a
/// second, and only a *failed* apply releases the gate for a retry.
#[test]
fn scheduler_serialises_applies() {
    // First tick applies and keeps the gate held (a restart is pending).
    let gate = AtomicBool::new(false);
    let (first, _c, applied) = tick(UpdateMode::Auto, IN_WINDOW, &gate, true);
    assert!(matches!(first, TickOutcome::Applied { .. }), "{first:?}");
    assert!(applied && gate.load(Ordering::SeqCst), "gate held after a successful apply");

    // A second close tick on the held gate must not launch a concurrent apply.
    let (second, checked, reapplied) = tick(UpdateMode::Auto, IN_WINDOW, &gate, true);
    assert!(matches!(second, TickOutcome::SkipInFlight), "{second:?}");
    assert!(checked && !reapplied, "no concurrent apply");

    assert_retry_after_failure();
}

/// A *failed* apply releases the gate so the next tick can retry.
fn assert_retry_after_failure() {
    let gate = AtomicBool::new(false);
    let failed = run_tick(
        &TickCtx { mode: UpdateMode::Auto, window: &MaintenanceWindow::default(), now_minutes: IN_WINDOW, apply_gate: &gate },
        || Ok(Some(fixture_manifest())),
        |_m| Err("download broke".to_owned()),
    );
    assert!(matches!(failed, TickOutcome::ApplyFailed(_)), "{failed:?}");
    assert!(!gate.load(Ordering::SeqCst), "failed apply releases the gate");
    let (retry, _c, applied) = tick(UpdateMode::Auto, IN_WINDOW, &gate, true);
    assert!(matches!(retry, TickOutcome::Applied { .. }), "{retry:?}");
    assert!(applied, "retry allowed after a failure");
}
