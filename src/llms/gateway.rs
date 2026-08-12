//! LLM gateway routing (`LiteLLM` and compatible proxies).
//!
//! `CP_LLM_GATEWAY` holds a base URL — `http://litellm:4000` in the container
//! deployment (`deploy/docker/docker-compose.yml`). Set, the API-key providers
//! post to that proxy and authenticate with `CP_LLM_GATEWAY_KEY`; unset or
//! empty, every client keeps its own endpoint constant and its own vault key,
//! byte for byte. The direct path costs one env read and nothing else.
//!
//! Two routes, because a proxy that rewrites bodies is not what this product
//! wants:
//!
//! - Anthropic-format clients target `<base>/anthropic/v1/messages`, `LiteLLM`'s
//!   **pass-through** route, which forwards the body unmodified — the only route
//!   that keeps provider-specific fields such as `cache_control` and the
//!   `anthropic-beta` header intact. Its auth is `Authorization: bearer <key>`
//!   rather than `x-api-key`: the single header difference between the direct
//!   and proxied paths.
//! - `OpenAI`-compatible clients target `<base>/v1/chat/completions`, the unified
//!   route, where `LiteLLM` resolves the body's `model` against its `model_list`.
//!   Every model string this product sends must be declared there — see
//!   `deploy/docker/litellm.yaml`.
//!
//! The two Claude Code providers ignore all of this on purpose; the reason is
//! documented where they build their requests.

use cp_base::config::llm_gateway;
use cp_mod_utilities::secret::Redacted;

/// Unified chat-completions path, appended to the base URL for every
/// `OpenAI`-compatible provider whatever its own path happens to be.
const OAI_PATH: &str = "/v1/chat/completions";

/// Where an Anthropic-format request goes, and how it authenticates.
pub(crate) struct AnthropicTarget {
    /// Full URL to POST to.
    pub url: String,
    /// Name of the auth header to set: `x-api-key` direct, `authorization`
    /// through the gateway.
    pub auth_header: &'static str,
    /// Value for that header.
    pub auth_value: Redacted,
}

/// Where an `OpenAI`-compatible request goes. The bearer header is applied by
/// [`run_oai_stream`](super::oai_providers::openai_streaming::run_oai_stream),
/// so only the URL and the key vary.
pub(crate) struct OaiTarget {
    /// Full URL to POST to.
    pub url: String,
    /// Bearer key for that URL.
    pub key: Redacted,
}

/// The configured gateway base URL, or `None` when unset or empty.
///
/// Delegates to [`cp_base::config::llm_gateway`], which the orchestrator also
/// reads to decide which providers the cockpit may offer. One definition, so the
/// picker and the request path cannot disagree.
fn base_url() -> Option<String> {
    llm_gateway::base_url()
}

/// The key to present to the gateway, falling back to the provider's own key
/// when `CP_LLM_GATEWAY_KEY` is unset. A proxy started without a master key
/// accepts any bearer value, so sending something it ignores beats refusing to
/// start.
///
/// When the key IS wrong, `LiteLLM` running without a database answers
/// `400 {"error":{"message":"No connected db.", …}}` — measured, not guessed. It
/// reads like a broken proxy and means "bad key"; there is no cleaner signal to
/// match on, which is why this fallback tries to send something plausible rather
/// than nothing.
fn gateway_key(direct_key: Option<&Redacted>) -> Redacted {
    if let Some(key) = std::env::var(llm_gateway::GATEWAY_KEY_ENV).ok().filter(|value| !value.trim().is_empty()) {
        return Redacted::new(key);
    }
    direct_key.map_or_else(|| Redacted::new("sk-no-gateway-key-set".to_owned()), Clone::clone)
}

/// Rewrite `direct_url`'s origin to `<base>/anthropic`, keeping path **and**
/// query — `…/v1/messages?beta=true` keeps its query string, which is what
/// makes the pass-through route usable by the beta endpoints too.
fn passthrough_url(base: &str, direct_url: &str) -> String {
    direct_url
        .split_once("://")
        .and_then(|(_scheme, rest)| rest.split_once('/'))
        .map_or_else(|| format!("{base}/anthropic/v1/messages"), |(_host, path)| format!("{base}/anthropic/{path}"))
}

/// Resolve the destination of an Anthropic-format request.
///
/// Through the gateway, `direct_key` is not consulted at all: a gateway
/// deployment holds the provider keys in the proxy, so the vault is legitimately
/// empty. Returns `None` only when there is no gateway *and* no local key, which
/// leaves the caller free to raise its own provider-specific error message.
pub(crate) fn anthropic_target(direct_url: &'static str, direct_key: Option<&Redacted>) -> Option<AnthropicTarget> {
    if let Some(base) = base_url() {
        return Some(AnthropicTarget {
            url: passthrough_url(&base, direct_url),
            auth_header: "authorization",
            auth_value: Redacted::new(format!("bearer {}", gateway_key(direct_key).expose_secret())),
        });
    }
    direct_key.map(|key| AnthropicTarget {
        url: direct_url.to_owned(),
        auth_header: "x-api-key",
        auth_value: key.clone(),
    })
}

/// Resolve the destination of an `OpenAI`-compatible request. Same rule as
/// [`anthropic_target`]; the auth header is `Authorization: Bearer` either way,
/// so only the URL and the key change.
pub(crate) fn oai_target(direct_url: &'static str, direct_key: Option<&Redacted>) -> Option<OaiTarget> {
    if let Some(base) = base_url() {
        return Some(OaiTarget { url: format!("{base}{OAI_PATH}"), key: gateway_key(direct_key) });
    }
    direct_key.map(|key| OaiTarget { url: direct_url.to_owned(), key: key.clone() })
}

/// Whether requests are currently routed through a gateway.
///
/// The `OpenAI`-compatible clients use this to ask for a final usage frame
/// (`stream_options.include_usage`): sent direct, xAI, Groq and `DeepSeek`
/// volunteer their usage in the last chunk, but a proxy is under no obligation
/// to, and a missing usage frame silently zeroes the cost log rather than
/// failing. The flag stays off on the direct path, where a provider that does
/// not know the field could reject the whole request.
pub(crate) fn is_active() -> bool {
    base_url().is_some()
}

#[cfg(test)]
#[path = "gateway_tests.rs"]
mod tests;
