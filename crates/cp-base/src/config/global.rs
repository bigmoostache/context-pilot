/// Global configuration at `~/.config/context-pilot/config.json`.
///
/// Provides centralized API key storage for all services (LLM providers,
/// web search, bridges). The resolution cascade: **env var → global config
/// → `None`**. Bridge-specific credentials (`TELEGRAM_API_ID`, etc.) also
/// live here since bridges are global infrastructure, not per-project.
use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// -- Well-known key names ----------------------------------------------------

/// Canonical key name → env var mappings for `resolve_api_key()`.
const KEY_ENV_MAP: &[(&str, &str)] = &[
    // LLM providers
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("deepseek", "DEEPSEEK_API_KEY"),
    ("xai", "XAI_API_KEY"),
    ("groq", "GROQ_API_KEY"),
    // Web tools
    ("brave", "BRAVE_API_KEY"),
    ("firecrawl", "FIRECRAWL_API_KEY"),
    // VCS
    ("github", "GITHUB_TOKEN"),
    // Bridge bot tokens
    ("telegram_bot", "TELEGRAM_BOT_TOKEN"),
    ("discord_bot", "DISCORD_BOT_TOKEN"),
    ("slack_bot", "SLACK_BOT_TOKEN"),
    ("googlechat_bot", "GOOGLECHAT_BOT_TOKEN"),
    // Bridge infrastructure (Telegram MTProto needs these)
    ("telegram_api_id", "TELEGRAM_API_ID"),
    ("telegram_api_hash", "TELEGRAM_API_HASH"),
];

// -- Config struct -----------------------------------------------------------

/// Serialized form of `~/.config/context-pilot/config.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// API keys and tokens, keyed by canonical name (e.g. `"anthropic"`,
    /// `"telegram_bot"`).
    #[serde(default)]
    pub keys: HashMap<String, String>,
}

// -- Paths -------------------------------------------------------------------

/// `~/.config/context-pilot/`
fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("context-pilot"))
}

/// `~/.config/context-pilot/config.json`
fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

// -- Read / write ------------------------------------------------------------

/// Load the global config, returning `Default` if missing or unparseable.
fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Persist `cfg` to disk with `chmod 600` (owner-only read/write).
fn save(cfg: &Config) -> Result<(), String> {
    let dir = config_dir().ok_or("Cannot determine XDG config directory")?;
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

// -- Public API --------------------------------------------------------------

/// Resolve a key by canonical name. Cascade: **env var → global config →
/// `None`**.
///
/// ```ignore
/// let key = resolve_api_key("anthropic");   // checks $ANTHROPIC_API_KEY first
/// let id  = resolve_api_key("telegram_api_id"); // checks $TELEGRAM_API_ID first
/// ```
#[must_use]
pub fn resolve_api_key(name: &str) -> Option<String> {
    // 1. Try env var (if a mapping exists)
    if let Some(env_var) = KEY_ENV_MAP.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
        && let Ok(val) = std::env::var(env_var)
    {
        let val = val.trim().to_owned();
        if !val.is_empty() {
            return Some(val);
        }
    }

    // 2. Try global config file
    let cfg = load();
    cfg.keys.get(name).filter(|v| !v.trim().is_empty()).cloned()
}

/// Store a key in the global config. Does **not** touch env vars.
///
/// # Errors
///
/// Returns a message if the config file cannot be written.
pub fn store_api_key(name: &str, value: &str) -> Result<(), String> {
    let mut cfg = load();
    drop(cfg.keys.insert(name.to_owned(), value.to_owned()));
    save(&cfg)
}

/// Return the env var name for a canonical key, if one exists.
#[must_use]
pub fn env_var_for_key(name: &str) -> Option<&'static str> {
    KEY_ENV_MAP.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}
