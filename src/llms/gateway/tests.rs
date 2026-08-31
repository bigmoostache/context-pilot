//! Tests for gateway routing.
//!
//! The URL rewriting is pure and always covered. The resolvers additionally read
//! the *process* environment, and `set_var` is `unsafe` on edition 2024
//! (`unsafe_code` is forbidden here), so those tests take their environment from
//! outside and are ignored by default:
//!
//! ```sh
//! CP_LLM_GATEWAY=http://litellm:4000 CP_LLM_GATEWAY_KEY=sk-test \
//!   cargo test -p tui --bins -- --ignored gateway
//! ```

use super::passthrough_url;

/// The plain Messages endpoint keeps its path under `/anthropic`.
#[test]
fn rewrites_origin_and_keeps_path() {
    assert_eq!(
        passthrough_url("http://litellm:4000", "https://api.anthropic.com/v1/messages"),
        "http://litellm:4000/anthropic/v1/messages"
    );
}

/// The query string survives — this is what would let the Claude Code endpoints
/// use the pass-through route if their auth is ever settled.
#[test]
fn keeps_query_string() {
    assert_eq!(
        passthrough_url("http://litellm:4000", "https://api.anthropic.com/v1/messages?beta=true"),
        "http://litellm:4000/anthropic/v1/messages?beta=true"
    );
}

/// A direct URL with no path at all still produces the documented route rather
/// than a malformed one.
#[test]
fn falls_back_when_direct_url_has_no_path() {
    assert_eq!(passthrough_url("http://gw", "https://api.anthropic.com"), "http://gw/anthropic/v1/messages");
}

/// Compare one resolved field, naming it so a failure says which one broke.
///
/// The two tests below return `Result` rather than asserting: a test that returns
/// `Result` may not also assert (clippy `panic_in_result_fn`), and `?` reports the
/// failure just as loudly.
fn same(field: &str, got: &str, want: &str) -> Result<(), String> {
    if got == want { Ok(()) } else { Err(format!("{field}: got {got:?}, want {want:?}")) }
}

/// Anthropic-format requests reach the pass-through route with a bearer header,
/// and resolve with `None` as the provider key: that is the vault of a deployment
/// which keeps its keys in the proxy.
#[test]
#[ignore = "needs CP_LLM_GATEWAY and CP_LLM_GATEWAY_KEY in the environment"]
fn resolves_the_anthropic_target_from_the_environment() -> Result<(), String> {
    if !super::is_active() {
        return Err("CP_LLM_GATEWAY must be set for this test".to_owned());
    }
    let target = super::anthropic_target("https://api.anthropic.com/v1/messages", None)
        .ok_or_else(|| "a configured gateway needs no provider key".to_owned())?;
    same("url", &target.url, "http://litellm:4000/anthropic/v1/messages")?;
    same("auth header", target.auth_header, "authorization")?;
    same("auth value", target.auth_value.expose_secret(), "bearer sk-test")
}

/// `OpenAI`-compatible requests reach the unified route, whatever path the
/// provider's own endpoint had.
#[test]
#[ignore = "needs CP_LLM_GATEWAY and CP_LLM_GATEWAY_KEY in the environment"]
fn resolves_the_oai_target_from_the_environment() -> Result<(), String> {
    let target = super::oai_target("https://api.x.ai/v1/chat/completions", None)
        .ok_or_else(|| "a configured gateway needs no provider key".to_owned())?;
    same("url", &target.url, "http://litellm:4000/v1/chat/completions")?;
    same("key", target.key.expose_secret(), "sk-test")
}

/// The default path: no gateway configured, so the client keeps the provider's
/// own URL and `x-api-key`. Skipped rather than failed when the environment does
/// have a gateway, so a run under `CP_LLM_GATEWAY` stays green.
#[test]
fn falls_back_to_the_direct_endpoint_without_a_gateway() -> Result<(), String> {
    if super::is_active() {
        return Ok(());
    }
    let key = cp_mod_utilities::secret::Redacted::new("sk-direct".to_owned());
    let target = super::anthropic_target("https://api.anthropic.com/v1/messages", Some(&key))
        .ok_or_else(|| "a provider key must resolve on the direct path".to_owned())?;
    same("url", &target.url, "https://api.anthropic.com/v1/messages")?;
    same("auth header", target.auth_header, "x-api-key")?;
    same("auth value", target.auth_value.expose_secret(), "sk-direct")?;
    if super::anthropic_target("https://api.anthropic.com/v1/messages", None).is_some() {
        return Err("no gateway and no key must not resolve".to_owned());
    }
    Ok(())
}
