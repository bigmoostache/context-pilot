/// Global configuration at `~/.config/context-pilot/config.json`.
///
/// Holds the central non-secret settings shared by every agent and user
/// (default provider/model, onboarding, access control). The `keys` map is a
/// legacy store kept for round-tripping older files; credentials are the
/// vault's business (`cp_vault::vault()`), not this module's.
use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// -- Config struct -----------------------------------------------------------

/// Serialized form of `~/.config/context-pilot/config.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Legacy API-key store, keyed by canonical name. No longer read or
    /// written; kept so older config files round-trip unchanged.
    #[serde(default)]
    pub keys: HashMap<String, String>,
    /// Non-secret central settings, keyed by name (e.g. `"default_provider"`,
    /// `"default_model"`, `"onboarding_completed"`). Server-side replacement
    /// for the cockpit's former localStorage defaults — set by the admin at
    /// onboarding and shared by all agents/users.
    #[serde(default)]
    pub settings: HashMap<String, String>,
}

// -- Paths -------------------------------------------------------------------

/// `~/.config/context-pilot/`
fn config_dir() -> PathBuf {
    cp_mod_utilities::dirs::config_dir().join("context-pilot")
}

/// `~/.config/context-pilot/config.json`
fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

// -- Read / write ------------------------------------------------------------

/// Load the global config, returning `Default` if missing or unparseable.
fn load() -> Config {
    let path = config_path();
    fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Persist `cfg` to disk with `chmod 600` (owner-only read/write).
fn save(cfg: &Config) -> Result<(), String> {
    let dir = config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("chmod {}: {e}", dir.display()))?;
    }
    let path = dir.join("config.json");
    let json = serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize: {e}"))?;
    let mut f = fs::File::create(&path).map_err(|e| format!("create {}: {e}", path.display()))?;
    f.write_all(json.as_bytes()).map_err(|e| format!("write {}: {e}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    Ok(())
}

// -- Central settings (non-secret) -------------------------------------------

/// Read a central setting by name, or `None` if unset/empty.
#[must_use]
pub fn get_setting(name: &str) -> Option<String> {
    load().settings.get(name).filter(|v| !v.trim().is_empty()).cloned()
}

/// Read every central setting as a map (non-secret values only).
#[must_use]
pub fn all_settings() -> HashMap<String, String> {
    load().settings
}

/// Persist a central setting. An empty `value` removes the key.
///
/// # Errors
///
/// Returns a message if the config file cannot be written.
pub fn set_setting(name: &str, value: &str) -> Result<(), String> {
    let mut cfg = load();
    if value.trim().is_empty() {
        drop(cfg.settings.remove(name));
    } else {
        drop(cfg.settings.insert(name.to_owned(), value.to_owned()));
    }
    save(&cfg)
}
