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

/// Load shared configuration from config.json.
///
/// A file that exists but doesn't parse is moved aside (see
/// [`quarantine_corrupt`]) before returning `None`, so the default state the
/// caller falls back to can't overwrite the only copy on the next save.
pub(crate) fn load_config() -> Option<SharedConfig> {
    load_json_or_quarantine(&config_path())
}

/// Read and parse a JSON state file. `None` when missing; when present but
/// unparsable, also quarantine it and log loudly.
pub(crate) fn load_json_or_quarantine<T>(path: &std::path::Path) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let json = fs::read_to_string(path).ok()?;
    match serde_json::from_str(&json) {
        Ok(value) => Some(value),
        Err(e) => {
            let moved = quarantine_corrupt(path);
            log::error!("corrupt state file {} ({e}); moved to {moved:?}, starting from defaults", path.display());
            None
        }
    }
}

/// Rename `path` to `<path>.corrupt-<unix_ms>` and return the new path.
pub(crate) fn quarantine_corrupt(path: &std::path::Path) -> Option<PathBuf> {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let mut name = path.file_name()?.to_os_string();
    name.push(format!(".corrupt-{ms}"));
    let target = path.with_file_name(name);
    fs::rename(path, &target).ok().map(|()| target)
}

#[cfg(test)]
mod tests {
    use super::load_json_or_quarantine;

    #[test]
    fn corrupt_file_is_quarantined_not_lost() {
        let dir = std::env::temp_dir().join(format!("cp-quarantine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("config.json");
        std::fs::write(&path, b"{\"active_theme\": \"da").expect("torn write");
        let loaded: Option<serde_json::Value> = load_json_or_quarantine(&path);
        assert!(loaded.is_none());
        assert!(!path.exists(), "corrupt file must be moved aside");
        let kept = std::fs::read_dir(&dir)
            .expect("read")
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with("config.json.corrupt-"));
        assert!(kept, "corrupt copy must be kept for recovery");
        let _cleanup = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none_without_side_effects() {
        let path = std::env::temp_dir().join("cp-quarantine-missing.json");
        let loaded: Option<serde_json::Value> = load_json_or_quarantine(&path);
        assert!(loaded.is_none());
    }
}
