//! [`Backend`] — orchestrator-backed credential backend with local cache
//! fallback.
//!
//! Used by agents running in bridge mode (`CP_BRIDGE=1`).  On boot the vault
//! fetches a bulk snapshot from the orchestrator and warms a local cache file.
//! A background thread re-fetches every 5 minutes so keys rotated on the
//! orchestrator propagate automatically.
//!
//! `get()` is **always local** — zero network latency on reads:
//!
//! 1. Orchestrator cache (populated from snapshot)
//! 2. [`Backend`](crate::local::Backend) (env vars, Keychain)
//!
//! If the orchestrator is unreachable at boot, the vault falls back to the
//! last-good disk cache (`~/.context-pilot/vault-cache.json`), then to env
//! vars loaded by dotenvy.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use crate::local::Backend as LocalVault;
use crate::registry::{ALL_KEYS, KeyCategory, resolve_definition};
use crate::types::{KeyStatus, SecretString, Vault, VaultError};

/// Default orchestrator URL when `CP_BRIDGE_URL` is not set.
const DEFAULT_ORCH_URL: &str = "http://127.0.0.1:7878";

/// Interval between background cache refreshes.
const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// HTTP connect timeout for orchestrator requests.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// HTTP total timeout for orchestrator requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Cache file name relative to `~/.context-pilot/`.
const CACHE_FILENAME: &str = "vault-cache.json";

/// Orchestrator-backed credential vault with cache fallback.
///
/// See [module docs](self) for the full resolution cascade.
pub struct Backend {
    /// Key→value cache populated from orchestrator snapshot.
    cache: Arc<RwLock<HashMap<String, SecretString>>>,
    /// Fallback backend for keys not in the orchestrator.
    local: LocalVault,
    /// Orchestrator base URL (e.g. `http://192.168.1.5:7878`).
    orch_url: String,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend").field("orch_url", &self.orch_url).finish_non_exhaustive()
    }
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend {
    /// Create a new bridge vault, warm cache from orchestrator, and start
    /// the background refresh thread.
    ///
    /// # Boot sequence
    ///
    /// 1. Try `GET /api/vault/snapshot` → populate in-memory cache + write
    ///    disk cache.
    /// 2. If orchestrator unreachable → load last-good disk cache.
    /// 3. Start background thread that re-fetches every 5 minutes.
    #[must_use]
    pub fn new() -> Self {
        let orch_url = std::env::var("CP_BRIDGE_URL").unwrap_or_else(|_| DEFAULT_ORCH_URL.to_owned());
        let local = LocalVault::new();
        let cache = Arc::new(RwLock::new(HashMap::new()));

        let vault = Self { cache, local, orch_url };

        // Boot: try orchestrator first, fall back to disk cache.
        if !vault.refresh_from_orchestrator() {
            vault.load_disk_cache();
        }

        vault.spawn_refresh_thread();
        vault
    }

    /// Fetch all keys from the orchestrator and update the in-memory cache.
    ///
    /// Returns `true` on success, `false` on any failure (network, parse).
    fn refresh_from_orchestrator(&self) -> bool {
        let url = format!("{}/api/vault/snapshot", self.orch_url);
        let Ok(client) =
            reqwest::blocking::Client::builder().connect_timeout(CONNECT_TIMEOUT).timeout(REQUEST_TIMEOUT).build()
        else {
            return false;
        };

        let response = match client.get(&url).send() {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                log::warn!("vault snapshot: HTTP {}", r.status());
                return false;
            }
            Err(e) => {
                log::warn!("vault snapshot unreachable: {e}");
                return false;
            }
        };

        let snapshot: BTreeMap<String, String> = match response.json() {
            Ok(m) => m,
            Err(e) => {
                log::warn!("vault snapshot: bad JSON: {e}");
                return false;
            }
        };

        // Update in-memory cache.
        if let Ok(mut guard) = self.cache.write() {
            guard.clear();
            for (k, v) in &snapshot {
                drop(guard.insert(k.clone(), SecretString::new(v.clone())));
            }
        }

        // Persist to disk for offline fallback.
        save_disk_cache(&snapshot);
        log::info!("vault snapshot: {} keys cached from orchestrator", snapshot.len());
        true
    }

    /// Load cached keys from `~/.context-pilot/vault-cache.json`.
    fn load_disk_cache(&self) {
        let Some(path) = cache_path() else { return };
        let Ok(content) = std::fs::read_to_string(&path) else { return };
        let map: BTreeMap<String, String> = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("vault cache corrupt, ignoring: {e}");
                return;
            }
        };
        if let Ok(mut guard) = self.cache.write() {
            for (k, v) in &map {
                // Only insert if not already present (orchestrator wins).
                if !guard.contains_key(k.as_str()) {
                    drop(guard.insert(k.clone(), SecretString::new(v.clone())));
                }
            }
        }
        log::info!("vault disk cache: loaded {} keys from {}", map.len(), path.display());
    }

    /// Spawn a daemon thread that refreshes the cache every
    /// [`REFRESH_INTERVAL`].
    fn spawn_refresh_thread(&self) {
        let cache = Arc::clone(&self.cache);
        let orch_url = self.orch_url.clone();

        let _handle = thread::Builder::new().name("vault-refresh".to_owned()).spawn(move || -> ! {
            loop {
                thread::sleep(REFRESH_INTERVAL);

                let url = format!("{orch_url}/api/vault/snapshot");
                let Ok(client) = reqwest::blocking::Client::builder()
                    .connect_timeout(CONNECT_TIMEOUT)
                    .timeout(REQUEST_TIMEOUT)
                    .build()
                else {
                    continue;
                };
                let response = match client.get(&url).send() {
                    Ok(r) if r.status().is_success() => r,
                    _ => continue,
                };
                let snapshot: BTreeMap<String, String> = match response.json() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if let Ok(mut guard) = cache.write() {
                    guard.clear();
                    for (k, v) in &snapshot {
                        drop(guard.insert(k.clone(), SecretString::new(v.clone())));
                    }
                }
                // Also persist to disk.
                save_disk_cache(&snapshot);
                log::debug!("vault refresh: {} keys updated", snapshot.len());
            }
        });
    }
}

impl Vault for Backend {
    fn get(&self, key: &str) -> Option<SecretString> {
        let def = resolve_definition(key)?;

        // 1. Orchestrator cache (highest priority in bridge mode).
        {
            let guard = self.cache.read().ok()?;
            if let Some(val) = guard.get(def.canonical) {
                return Some(val.clone());
            }
        }

        // 2. Local fallback (env vars, Keychain).
        self.local.get(key)
    }

    fn require(&self, key: &str) -> Result<SecretString, VaultError> {
        self.get(key).ok_or_else(|| VaultError::MissingKey(key.to_owned()))
    }

    fn set(&self, key: &str, value: &str) -> Result<(), VaultError> {
        let def = resolve_definition(key).ok_or_else(|| VaultError::MissingKey(format!("unknown key: {key}")))?;

        // Try pushing to orchestrator.
        let url = format!("{}/api/env-keys/{}", self.orch_url, def.canonical);
        let body = serde_json::json!({ "value": value });

        let push_ok = reqwest::blocking::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok()
            .and_then(|c| c.put(&url).json(&body).send().ok())
            .is_some_and(|r| r.status().is_success());

        if !push_ok {
            log::warn!("vault set: could not push {key} to orchestrator, storing locally");
        }

        // Always store in local cache for immediate availability.
        if let Ok(mut guard) = self.cache.write() {
            drop(guard.insert(def.canonical.to_owned(), SecretString::new(value.to_owned())));
        }

        // Also persist locally.
        self.local.set(key, value)
    }

    fn delete(&self, key: &str) -> Result<(), VaultError> {
        let def = resolve_definition(key).ok_or_else(|| VaultError::MissingKey(format!("unknown key: {key}")))?;

        if let Ok(mut guard) = self.cache.write() {
            drop(guard.remove(def.canonical));
        }

        self.local.delete(key)
    }

    fn list(&self) -> Vec<KeyStatus> {
        ALL_KEYS
            .iter()
            .map(|def| {
                let available = self.get(def.canonical).is_some();
                KeyStatus { definition: def, available }
            })
            .collect()
    }

    fn health(&self) -> Vec<&'static crate::registry::KeyDefinition> {
        ALL_KEYS
            .iter()
            .filter(|def| {
                matches!(def.category, KeyCategory::LlmProvider | KeyCategory::WebTool | KeyCategory::Vcs)
                    && self.get(def.canonical).is_none()
            })
            .collect()
    }
}

/// Write the snapshot to disk with restrictive permissions.
fn save_disk_cache(snapshot: &BTreeMap<String, String>) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _created = std::fs::create_dir_all(parent);
    }
    let Ok(json) = serde_json::to_string_pretty(snapshot) else { return };
    if std::fs::write(&path, &json).is_ok() {
        secure_file(&path);
    }
}

/// Resolve the disk cache path: `~/.context-pilot/vault-cache.json`.
fn cache_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(std::path::PathBuf::from(home).join(".context-pilot").join(CACHE_FILENAME))
}

/// Set file permissions to 0600 (owner read/write only).
#[cfg(unix)]
fn secure_file(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let _set = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// No-op on non-unix platforms.
#[cfg(not(unix))]
fn secure_file(_path: &std::path::Path) {}
