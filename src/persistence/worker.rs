//! Worker state persistence module
//! Handles loading and saving worker state files (states/{worker}.json)
use std::fs;
use std::path::PathBuf;

use crate::constants::{STATES_DIR, STORE_DIR};
use crate::state::WorkerState;

fn states_dir() -> PathBuf {
    PathBuf::from(STORE_DIR).join(STATES_DIR)
}

fn worker_path(worker_id: &str) -> PathBuf {
    states_dir().join(format!("{}.json", worker_id))
}

/// Load worker state from states/{worker_id}.json
pub fn load_worker(worker_id: &str) -> Option<WorkerState> {
    let path = worker_path(worker_id);
    let json = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&json).ok()
}
