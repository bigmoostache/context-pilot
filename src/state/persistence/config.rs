//! Shared configuration persistence module
//! Handles loading and saving config.json (global settings)
use std::fs;
use std::path::PathBuf;
use std::process;

use crate::infra::constants::{CONFIG_FILE, STORE_DIR};
use crate::state::SharedConfig;

/// Build the filesystem path to the shared config file.
fn config_path() -> PathBuf {
    PathBuf::from(STORE_DIR).join(CONFIG_FILE)
}

/// Get current process PID
pub(crate) fn current_pid() -> u32 {
    process::id()
}

/// Load shared configuration from config.json
pub(crate) fn load_config() -> Option<SharedConfig> {
    let path = config_path();
    let json = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&json).ok()
}
