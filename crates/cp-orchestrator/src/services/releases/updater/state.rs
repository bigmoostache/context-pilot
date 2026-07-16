//! Durable updater state — `releases/update-state.json`.
//!
//! What the cockpit's *Update* pane (M5) reads: when the box last checked the
//! channel, what version (if any) is on offer, and how the last apply ended.
//! Written atomically (tmp → rename), tolerant of absence/corruption (defaults
//! win — the updater never fails a boot over its own status file).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name under the releases directory.
const STATE_FILE: &str = "update-state.json";

/// How the last apply attempt ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateResult {
    /// The staged version booted healthy and was committed.
    Success {
        /// Version running before the update (`None` on a first install).
        from: Option<String>,
        /// Version now active.
        to: String,
        /// Epoch-ms of the commit.
        at_ms: u64,
    },
    /// The staged version never turned healthy — the box reverted.
    RolledBack {
        /// Version the box rolled back **to** (the pre-update one).
        to: Option<String>,
        /// Version whose apply failed.
        attempted: String,
        /// Epoch-ms of the reconciliation.
        at_ms: u64,
    },
    /// The apply pipeline failed before any restart (download, staging…).
    Failed {
        /// Human-readable reason.
        message: String,
        /// Epoch-ms of the failure.
        at_ms: u64,
    },
}

/// The durable updater state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    /// Epoch-ms of the last channel check (successful or not).
    pub last_check_ms: Option<u64>,
    /// Version the channel offers, as of the last *verified* check.
    pub available: Option<String>,
    /// Release-notes URL for the offered version (from the manifest).
    pub available_notes_url: Option<String>,
    /// Outcome of the most recent apply attempt.
    pub last_result: Option<UpdateResult>,
}

impl UpdateState {
    /// Load the state from `releases_dir`, defaulting on absence/corruption.
    #[must_use]
    pub fn load(releases_dir: &Path) -> Self {
        std::fs::read(state_path(releases_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Atomically persist the state under `releases_dir` (best-effort).
    pub fn save(&self, releases_dir: &Path) {
        let Ok(bytes) = serde_json::to_vec_pretty(self) else {
            return;
        };
        if std::fs::create_dir_all(releases_dir).is_err() {
            return;
        }
        let path = state_path(releases_dir);
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &bytes).is_ok() {
            let _renamed = std::fs::rename(&tmp, &path);
        }
    }
}

/// Path of the state file under `releases_dir`.
fn state_path(releases_dir: &Path) -> PathBuf {
    releases_dir.join(STATE_FILE)
}

/// Current time as epoch milliseconds (0 if the clock is before 1970).
#[must_use]
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Current time as epoch seconds (0 if the clock is before 1970).
#[must_use]
pub(crate) fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
