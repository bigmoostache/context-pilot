//! Unified credential vault — single source of truth for all API keys and secrets.
//!
//! # Architecture
//!
//! The vault provides a single [`Vault`](types::Vault) trait with two backends:
//! - [`Backend`](local::Backend) — resolves keys from environment variables,
//!   macOS Keychain, and credential files.  Used by standalone agents and the
//!   orchestrator.
//! - `BridgeVault` (feature-gated) — fetches from the orchestrator with local
//!   cache fallback.  Used by agents in bridge mode.
//!
//! # Initialization
//!
//! The vault auto-initializes on first access via [`LazyLock`].  Ensure that
//! `dotenvy` has loaded `.env` files before any module calls [`vault()`].
//!
//! # Key Resolution
//!
//! Keys can be referenced by canonical name (`"anthropic"`) or env var name
//! (`"ANTHROPIC_API_KEY"`) — both resolve identically.

mod dotenv;
pub mod local;
mod oauth;
pub mod registry;
pub mod types;

use std::sync::{Arc, LazyLock};

use types::Vault;

/// Global vault instance, auto-initialized on first access.
///
/// Backend selection reads `CP_BRIDGE` at initialization time.
/// Currently always creates a [`local::Backend`]; `BridgeVault` will be added
/// behind the `bridge` feature flag.
static VAULT: LazyLock<Arc<dyn Vault>> = LazyLock::new(|| Arc::new(local::Backend::new()));

/// Access the global vault instance.
///
/// First call triggers initialization (reads `CP_BRIDGE` env var to select
/// backend).  Subsequent calls return the cached reference with zero overhead.
#[must_use]
pub fn vault() -> &'static Arc<dyn Vault> {
    &VAULT
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn vault_auto_initializes() {
        let v = vault();
        // Should return a valid vault without explicit init.
        let statuses = v.list();
        assert!(!statuses.is_empty());
    }

    #[test]
    fn vault_returns_same_instance() {
        let a: *const Arc<dyn Vault> = vault();
        let b: *const Arc<dyn Vault> = vault();
        assert_eq!(a, b);
    }
}
