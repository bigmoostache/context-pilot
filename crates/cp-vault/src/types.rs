//! Core vault types — [`SecretString`], [`VaultError`], [`KeyStatus`], and the [`Vault`] trait.

use std::fmt;

use zeroize::Zeroizing;

use crate::registry::KeyDefinition;

// ─── SecretString ───────────────────────────────────────────────────────────

/// A string that is zeroized from memory on drop.
///
/// No `Display` or useful `Debug` — prevents accidental logging of secrets.
/// Access the inner value only via [`SecretString::expose()`].
#[derive(Clone)]
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    /// Wrap a plaintext value in a zeroizing container.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// Expose the secret value.
    ///
    /// Use sparingly — only when the value must be passed to an external API
    /// (HTTP headers, process env, etc.).
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

// ─── VaultError ─────────────────────────────────────────────────────────────

/// Errors that can occur during vault operations.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "vault error taxonomy is a closed set (MissingKey/Io/Network) constructed within cp-vault and matched exhaustively by callers; #[non_exhaustive] would force cross-crate wildcard arms that the forbidden wildcard_enum_match_arm lint rejects"
)]
pub enum VaultError {
    /// Requested key is not configured anywhere in the resolution cascade.
    MissingKey(String),
    /// File I/O failure (reading `.env`, writing cache, etc.).
    Io(String),
    /// Network failure (bridge mode only).
    Network(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey(key) => write!(f, "credential not configured: {key}"),
            Self::Io(msg) => write!(f, "vault I/O error: {msg}"),
            Self::Network(msg) => write!(f, "vault network error: {msg}"),
        }
    }
}

// ─── KeyStatus ──────────────────────────────────────────────────────────────

/// Status of a single key as reported by [`Vault::list()`].
#[derive(Debug, Clone, Copy)]
pub struct KeyStatus {
    /// The key's static definition from the registry.
    pub definition: &'static KeyDefinition,
    /// Whether the key currently resolves to a value.
    pub available: bool,
}

// ─── Vault trait ────────────────────────────────────────────────────────────

/// Unified credential access — the single API every module uses for secrets.
///
/// Implementations: [`crate::local::Backend`] (standalone), `bridge::Backend`
/// (orchestrator-backed with cache fallback, feature-gated).
pub trait Vault: Send + Sync + fmt::Debug {
    /// Resolve a key by canonical name or env var name.
    ///
    /// Returns `None` if the key is not configured.
    fn get(&self, key: &str) -> Option<SecretString>;

    /// Like [`Vault::get()`] but returns an error on missing keys.
    ///
    /// Preferred in code paths where the key is required for operation.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::MissingKey`] if the key is not configured.
    fn require(&self, key: &str) -> Result<SecretString, VaultError> {
        self.get(key).ok_or_else(|| VaultError::MissingKey(key.to_owned()))
    }

    /// Store a key value.
    ///
    /// Persists to `~/.context-pilot/.env` and updates the in-memory override.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Io`] on file write failure, or
    /// [`VaultError::MissingKey`] if the key is not in the registry.
    fn set(&self, key: &str, value: &str) -> Result<(), VaultError>;

    /// Remove a key from the in-memory override.
    ///
    /// Does NOT remove from `.env` — only clears the runtime override.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::MissingKey`] if the key is not in the registry.
    fn delete(&self, key: &str) -> Result<(), VaultError>;

    /// List all known keys with their availability status.
    fn list(&self) -> Vec<KeyStatus>;

    /// Return definitions for keys that are required but missing.
    ///
    /// Used at boot for health-check warnings.
    fn health(&self) -> Vec<&'static KeyDefinition>;
}
