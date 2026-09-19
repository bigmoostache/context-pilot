//! Provider usability — the per-provider "can it answer right now?" verdict
//! behind `GET /api/providers`.
//!
//! Split out of [`super`] so the catalogue module stays within the file budget.

use cp_env::model::features::Feature;

use super::{gateway_models, oauth_creds};
use crate::transport::rest;

/// Is provider `id` usable right now? API-key providers need their key
/// configured; the Claude Code OAuth backends instead need a present,
/// non-expired credentials file (provisioned out-of-band — see
/// `deploy/ansible/claude-oauth.yml`).
///
/// Key presence is resolved through the [`cp_vault`] credential vault — the same
/// store the settings page writes to (`vault.set()`). This matters because the
/// vault reflects keys added **at runtime** (in-memory override + a direct
/// re-read of `~/.context-pilot/.env`), whereas `global::has_api_key` only sees
/// process env vars loaded by dotenvy at boot. Reading the vault here keeps the
/// picker in sync with the key manager without requiring an orchestrator
/// restart. A gateway short-circuits all of it — see [`gateway_models::provides`].
pub(super) fn provider_usable(id: &str) -> bool {
    if gateway_models::provides(id) {
        return true;
    }
    match id {
        "claudecodev2" => rest::features().is_on(Feature::ClaudeOauth) && oauth_creds::claude_oauth_available(),
        _ => provider_key_name(id).is_some_and(|key| cp_vault::vault().get(key).is_some()),
    }
}

/// Map a catalogue provider id to the central key name used to check usability.
/// Returns `None` for providers with no API-key path (the Claude Code OAuth
/// backends — their usability is decided by [`claude_oauth_available`]).
fn provider_key_name(id: &str) -> Option<&'static str> {
    match id {
        "anthropic" => Some("anthropic"),
        "grok" => Some("xai"),
        "groq" => Some("groq"),
        "deepseek" => Some("deepseek"),
        "minimax" => Some("minimax"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_providers_route_through_the_oauth_check_not_an_api_key() {
        // The OAuth backend must never be gated on an API-key name.
        assert_eq!(provider_key_name("claudecodev2"), None);
    }
}
