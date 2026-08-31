//! Optional LLM gateway: is one configured, and whose traffic does it carry?
//!
//! Two crates need the same answer and must not drift apart. The agent asks in
//! order to build a request (`src/llms/gateway/mod.rs` resolves the URL and the
//! auth header from it), and the orchestrator asks in order to decide which
//! providers the cockpit may offer — under a gateway a provider is usable with
//! **no local key at all**, because the key that matters lives in the proxy.
//! Duplicating either the variable name or the routed-provider list would make
//! the picker and the request path disagree, which reads as "no models
//! available" with no error anywhere.

/// Base URL of the gateway. Absent or empty means "no gateway".
pub const GATEWAY_URL_ENV: &str = "CP_LLM_GATEWAY";

/// Key presented to the gateway on every call.
pub const GATEWAY_KEY_ENV: &str = "CP_LLM_GATEWAY_KEY";

/// Catalogue ids of the providers a gateway carries.
///
/// The absentees are deliberate. `claudecodev2` authenticates with a Claude Code
/// subscription OAuth token, which cannot survive a proxy that substitutes its own
/// credential; `minimax` speaks the Anthropic format on its own domain, which no
/// pass-through route reaches. Both keep talking to their own API and stay gated
/// on their own local credential.
pub const GATEWAY_PROVIDERS: &[&str] = &["anthropic", "grok", "groq", "deepseek"];

/// Catalogue ids whose availability the gateway's `GET /v1/models` can attest.
///
/// A subset of [`GATEWAY_PROVIDERS`], and the distinction is load-bearing. Only
/// the unified chat-completions route resolves the body's `model` against the
/// proxy's `model_list`, so only those providers are named in `/v1/models`.
/// `anthropic` takes the pass-through route, which forwards to Anthropic without
/// consulting that table at all — its models are never declared there, and
/// filtering it on that list would empty it out. Its reachability is therefore
/// undecidable from `/v1/models`; only the proxy's own `/health` knows, and that
/// costs a real request per model.
pub const MODEL_LIST_PROVIDERS: &[&str] = &["grok", "groq", "deepseek"];

/// The configured gateway base URL, trailing slashes trimmed.
///
/// `None` when unset or empty: empty counts as unset so an operator can disable
/// the gateway by blanking the variable rather than deleting the line.
#[must_use]
pub fn base_url() -> Option<String> {
    let raw = std::env::var(GATEWAY_URL_ENV).ok()?;
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

/// Whether a gateway is configured at all.
#[must_use]
pub fn is_active() -> bool {
    base_url().is_some()
}

/// Whether `provider_id` (a catalogue id, as served by `GET /api/providers`)
/// reaches its models through the gateway when one is configured.
#[must_use]
pub fn routes_provider(provider_id: &str) -> bool {
    GATEWAY_PROVIDERS.contains(&provider_id)
}

/// Whether `provider_id`'s models are named in the gateway's `GET /v1/models`,
/// and can therefore be filtered against it. See [`MODEL_LIST_PROVIDERS`].
#[must_use]
pub fn declared_in_model_list(provider_id: &str) -> bool {
    MODEL_LIST_PROVIDERS.contains(&provider_id)
}
