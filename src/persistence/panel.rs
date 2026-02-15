//! Panel data persistence module
//! Handles loading and saving panel files (panels/{uid}.json)
//! Includes conversation panels and dynamic panels (File, Glob, Grep, Tmux)
use std::fs;
use std::path::PathBuf;

use crate::constants::{PANELS_DIR, STORE_DIR};
use crate::state::PanelData;

fn panels_dir() -> PathBuf {
    PathBuf::from(STORE_DIR).join(PANELS_DIR)
}

fn panel_path(uid: &str) -> PathBuf {
    panels_dir().join(format!("{}.json", uid))
}

/// Load panel data by UID from panels/{uid}.json
pub fn load_panel(uid: &str) -> Option<PanelData> {
    let path = panel_path(uid);
    let json = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&json).ok()
}
