//! Scheduler core — one poll tick's decision logic (O4.2), kept pure so the
//! clock, the channel check and the apply pipeline are all injected. The
//! runtime glue (locks, sleeps, restart) lives in
//! `runtime::update_scheduler`; this module owns *what happens on a tick*:
//!
//! * every mode **checks** (state stays fresh for the cockpit);
//! * only `auto`, **inside the box-local window**, with a verified update,
//!   **applies** — `manual`/`paused` never do;
//! * applies are serialised by a caller-owned gate: once one is in flight
//!   (the process is about to re-exec) no second one can start.

use std::sync::atomic::{AtomicBool, Ordering};

use super::super::config::{MaintenanceWindow, UpdateMode, parse_hhmm};
use super::super::{Manifest, ReleaseStore};

/// What one tick did — one structured log line per decision (T4.2.3).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TickOutcome {
    /// The channel check itself failed (network, signature…): state kept.
    CheckFailed(String),
    /// Verified: the box already runs the channel version.
    UpToDate,
    /// Update available but the mode forbids automatic applies.
    SkipMode(UpdateMode),
    /// Update available, mode `auto`, but outside the maintenance window.
    SkipWindow { available: String },
    /// Update available but an apply is already in flight (restart pending).
    SkipInFlight,
    /// The apply pipeline was launched (download + stage + restart).
    Applied { from: String, to: String },
    /// The apply pipeline failed; the gate was released for a retry.
    ApplyFailed(String),
}

impl TickOutcome {
    /// The log line for this decision.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::CheckFailed(e) => format!("check failed: {e}"),
            Self::UpToDate => "up to date".to_owned(),
            Self::SkipMode(mode) => format!("skip: mode is {}", mode.as_str()),
            Self::SkipWindow { available } => format!("skip: {available} available but outside the window"),
            Self::SkipInFlight => "skip: an apply is already in flight".to_owned(),
            Self::Applied { from, to } => format!("apply: {from} → {to}"),
            Self::ApplyFailed(e) => format!("apply failed: {e}"),
        }
    }
}

/// Run one scheduler tick.
///
/// `check` fetches + verifies the channel (recording durable state) and
/// returns the available manifest, if any. `apply` runs the M3 pipeline
/// (download → stage → restart). `apply_gate` serialises applies: it is
/// acquired here and **released only on apply failure** — a successful apply
/// ends in a process restart, so the gate deliberately stays held.
pub(crate) fn run_tick<C, A>(
    mode: UpdateMode,
    window: &MaintenanceWindow,
    now_minutes: u16,
    apply_gate: &AtomicBool,
    check: C,
    apply: A,
) -> TickOutcome
where
    C: FnOnce() -> Result<Option<Manifest>, String>,
    A: FnOnce(&Manifest) -> Result<String, String>,
{
    // Every mode checks — the cockpit's "available vY" stays fresh even when
    // applies are off (T4.2.2).
    let manifest = match check() {
        Err(e) => return TickOutcome::CheckFailed(e),
        Ok(None) => return TickOutcome::UpToDate,
        Ok(Some(manifest)) => manifest,
    };

    match mode {
        UpdateMode::Manual | UpdateMode::Paused => return TickOutcome::SkipMode(mode),
        UpdateMode::Auto => {}
    }
    if !window.contains(now_minutes) {
        return TickOutcome::SkipWindow { available: manifest.version };
    }
    if apply_gate.swap(true, Ordering::SeqCst) {
        return TickOutcome::SkipInFlight;
    }
    match apply(&manifest) {
        Ok(from) => TickOutcome::Applied { from, to: manifest.version },
        Err(e) => {
            apply_gate.store(false, Ordering::SeqCst); // allow a retry next tick
            TickOutcome::ApplyFailed(e)
        }
    }
}

/// The version this box currently runs — the active release tag when the
/// binaries came from the store, else the build's own version (day-0 installs
/// laid down by Ansible have no active tag yet).
#[must_use]
pub(crate) fn current_version(store: &ReleaseStore) -> String {
    store.active_tag().map_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")), str::to_owned)
}

/// Minutes since box-local midnight. The box's wall clock is asked via
/// `date +%H:%M` (std has no timezone database); UTC is the fallback so a
/// missing `date` binary degrades to a shifted window, never a panic.
#[must_use]
pub(crate) fn local_now_minutes() -> u16 {
    let local = std::process::Command::new("date")
        .arg("+%H:%M")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|s| parse_hhmm(s.trim()));
    local.unwrap_or_else(|| {
        let secs = super::state::now_epoch_secs();
        u16::try_from(secs % 86_400 / 60).unwrap_or(0)
    })
}
