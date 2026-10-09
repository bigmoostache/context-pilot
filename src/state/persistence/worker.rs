//! Worker state persistence module
//!
//! Handles loading and saving worker state files. One file per thread, named
//! `states/<thread_id>.json`; an agent with no focused thread keeps the legacy
//! `states/main_worker.json` (see [`focused_worker_id`]).
use std::fs;
use std::path::PathBuf;

use crate::infra::constants::{DEFAULT_WORKER_ID, STATES_DIR, STORE_DIR};
use crate::state::WorkerState;

/// Build the path to the worker states directory.
fn states_dir() -> PathBuf {
    PathBuf::from(STORE_DIR).join(STATES_DIR)
}

/// Build the filesystem path for a worker with the given ID.
fn worker_path(worker_id: &str) -> PathBuf {
    states_dir().join(format!("{worker_id}.json"))
}

/// Whether `states/{worker_id}.json` exists on disk.
pub(crate) fn exists(worker_id: &str) -> bool {
    worker_path(worker_id).is_file()
}

/// Load worker state from `states/{worker_id}.json`
pub(crate) fn load_worker(worker_id: &str) -> Option<WorkerState> {
    let path = worker_path(worker_id);
    let json = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&json).ok()
}

/// The state-file id boot must load: the thread whose context was last saved
/// focused.
///
/// This is the read half of the write half
/// ([`executing_worker_id`](super::save::executing_worker_id)), and the two must
/// agree or a reload silently restores the wrong conversation. `focus_file` is
/// the pointer that save records in the shared config, because the focused
/// thread's own id lives *inside* the file this function chooses — boot cannot
/// read it without already knowing it.
///
/// `exists` is injected rather than calling [`exists`] directly so the rule can
/// be tested without a temp directory and a real store layout.
///
/// The fallback to [`DEFAULT_WORKER_ID`] covers two cases, both one-shot:
/// - no pointer at all — an install saved before pointers existed, or an
///   unfocused agent (boot before any `Read`) which has no thread to name;
/// - a pointer whose file is missing — the save that recorded the pointer
///   crashed before its keyed write landed, so the legacy file is still the
///   only complete copy.
///
/// Preferring the legacy file over loading nothing is deliberate: a stale
/// context is recoverable, an empty one loses the conversation outright.
pub(crate) fn focused_worker_id(focus_file: Option<&str>, exists: impl Fn(&str) -> bool) -> String {
    if let Some(id) = focus_file.filter(|id| !id.is_empty())
        && exists(id)
    {
        return id.to_owned();
    }
    DEFAULT_WORKER_ID.to_owned()
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_WORKER_ID, focused_worker_id};

    /// Probe that reports only the named ids as present.
    fn present(ids: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |id| ids.contains(&id)
    }

    #[test]
    fn no_pointer_falls_back_to_legacy() {
        assert_eq!(focused_worker_id(None, present(&[])), DEFAULT_WORKER_ID);
    }

    #[test]
    fn empty_pointer_falls_back_to_legacy() {
        assert_eq!(focused_worker_id(Some(""), present(&["T774"])), DEFAULT_WORKER_ID);
    }

    #[test]
    fn keyed_file_present_is_selected() {
        assert_eq!(focused_worker_id(Some("T774"), present(&["T774"])), "T774");
    }

    #[test]
    fn missing_keyed_file_falls_back_to_legacy() {
        // Pointer recorded but its write never landed: the legacy file is the
        // only complete copy, so it wins over loading nothing.
        assert_eq!(focused_worker_id(Some("T774"), present(&[DEFAULT_WORKER_ID])), DEFAULT_WORKER_ID);
    }

    #[test]
    fn missing_keyed_file_with_no_legacy_still_named_legacy() {
        // Nothing on disk at all: naming the legacy id keeps `load_worker`
        // returning None, which callers already treat as a fresh start.
        assert_eq!(focused_worker_id(Some("T774"), present(&[])), DEFAULT_WORKER_ID);
    }
}
