//! [`Backend`] — standalone credential backend.
//!
//! Resolution cascade for `get(key)`:
//! 1. In-memory overrides (set via [`Vault::set()`])
//! 2. Process environment (`std::env::var`) — loaded by dotenvy at boot
//! 3. Keychain / credential file (for OAuth keys with [`AuthMechanism::KeychainThenFile`])
//!
//! Used by agents in standalone mode and by the orchestrator.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::dotenv;
use crate::oauth;
use crate::registry::{ALL_KEYS, AuthMechanism, KeyCategory, KeyDefinition, resolve_definition};
use crate::types::{KeyStatus, SecretString, Vault, VaultError};

/// Standalone vault backend — resolves from env vars, overrides, and Keychain.
#[derive(Debug)]
pub struct Backend {
    /// Hot-set values that override env vars.  Set via [`Vault::set()`].
    overrides: RwLock<HashMap<String, SecretString>>,
}

impl Backend {
    /// Create a new standalone vault backend with empty overrides.
    ///
    /// The vault immediately resolves keys from process environment variables
    /// (populated by dotenvy at boot) and the macOS Keychain.
    #[must_use]
    pub fn new() -> Self {
        Self { overrides: RwLock::new(HashMap::new()) }
    }

    /// Resolve a key using the full cascade (overrides → env → keychain/file).
    fn resolve(&self, def: &KeyDefinition) -> Option<SecretString> {
        // 1. In-memory override (highest priority).
        {
            let guard = self.overrides.read().ok()?;
            if let Some(val) = guard.get(def.canonical) {
                return Some(val.clone());
            }
        }

        // 2. Environment variable (loaded by dotenvy at boot).
        if !def.env_var.is_empty()
            && let Ok(val) = std::env::var(def.env_var)
            && !val.is_empty()
        {
            return Some(SecretString::new(val));
        }

        // 3. Mechanism-specific fallback (Keychain / credential file).
        match def.mechanism {
            AuthMechanism::KeychainThenFile => oauth::load_claude_oauth_token(),
            AuthMechanism::EnvVar => {
                // Also try reading directly from .env file (covers cases where
                // dotenvy didn't load, e.g. key added after boot).
                if def.env_var.is_empty() { None } else { dotenv::read_env_key(def.env_var).map(SecretString::new) }
            }
        }
    }
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Vault for Backend {
    fn get(&self, key: &str) -> Option<SecretString> {
        let def = resolve_definition(key)?;
        self.resolve(def)
    }

    fn require(&self, key: &str) -> Result<SecretString, VaultError> {
        self.get(key).ok_or_else(|| VaultError::MissingKey(key.to_owned()))
    }

    fn set(&self, key: &str, value: &str) -> Result<(), VaultError> {
        let def = resolve_definition(key).ok_or_else(|| VaultError::MissingKey(format!("unknown key: {key}")))?;

        // Persist to ~/.context-pilot/.env (only for env-var-backed keys).
        if !def.env_var.is_empty() {
            dotenv::write_env_entry(def.env_var, value)?;
        }

        // Store in-memory override for immediate availability.
        let mut guard = self.overrides.write().map_err(|e| VaultError::Io(e.to_string()))?;
        drop(guard.insert(def.canonical.to_owned(), SecretString::new(value.to_owned())));
        drop(guard);

        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), VaultError> {
        let def = resolve_definition(key).ok_or_else(|| VaultError::MissingKey(format!("unknown key: {key}")))?;

        let mut guard = self.overrides.write().map_err(|e| VaultError::Io(e.to_string()))?;
        drop(guard.remove(def.canonical));
        drop(guard);

        Ok(())
    }

    fn list(&self) -> Vec<KeyStatus> {
        ALL_KEYS.iter().map(|def| KeyStatus { definition: def, available: self.resolve(def).is_some() }).collect()
    }

    fn health(&self) -> Vec<&'static KeyDefinition> {
        // Report missing keys that are in "important" categories.
        ALL_KEYS
            .iter()
            .filter(|def| {
                matches!(def.category, KeyCategory::LlmProvider | KeyCategory::WebTool | KeyCategory::Vcs)
                    && self.resolve(def).is_none()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_takes_precedence() {
        let vault = Backend::new();
        let result = vault.set("anthropic", "sk-override-123");
        assert!(result.is_ok());

        let val = vault.get("anthropic");
        assert!(val.is_some());
        assert_eq!(val.map(|s| s.expose().to_owned()), Some("sk-override-123".to_owned()));
    }

    #[test]
    fn delete_removes_override() {
        let vault = Backend::new();
        let _r = vault.set("anthropic", "sk-temp");
        let _r = vault.delete("anthropic");

        let guard = vault.overrides.read().unwrap_or_else(|e| e.into_inner());
        assert!(!guard.contains_key("anthropic"));
    }

    #[test]
    fn unknown_key_returns_error() {
        let vault = Backend::new();
        let result = vault.set("totally_fake_key_xyz", "val");
        assert!(result.is_err());
    }

    #[test]
    fn list_returns_all_keys() {
        let vault = Backend::new();
        let statuses = vault.list();
        assert_eq!(statuses.len(), ALL_KEYS.len());
    }

    #[test]
    fn dual_name_resolution() {
        let vault = Backend::new();
        let _r = vault.set("brave", "key-123");

        let by_canonical = vault.get("brave");
        let by_env = vault.get("BRAVE_API_KEY");

        assert_eq!(by_canonical.map(|s| s.expose().to_owned()), Some("key-123".to_owned()));
        assert_eq!(by_env.map(|s| s.expose().to_owned()), Some("key-123".to_owned()));
    }
}
