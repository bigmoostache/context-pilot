//! LLM provider + model registry — the single source of truth for all provider
//! and model metadata surfaced to the frontend (model picker, config panel,
//! per-agent manage modal).
//!
//! The registry mirrors the TUI's `LlmProvider` / `ModelInfo` enums. Keeping it
//! here (rather than in `cp-wire`) avoids coupling the shared protocol crate to
//! pricing/display metadata that only the orchestrator and frontend need.
//!
//! The frontend imports `ProviderDef` and `ModelDef` from the generated `OpenAPI`
//! client and fetches the data via `GET /api/providers` — zero hardcoded model
//! lists in TypeScript.

use serde::Serialize;

use crate::transport::rest;
use crate::transport::rest::HttpReply;

/// What the LLM gateway declares it can route (`GET /v1/models`), cached.
mod gateway_models;
mod oauth_creds;

// ── Wire types ──────────────────────────────────────────────────────────

/// A selectable LLM provider and its catalogue of models, as surfaced to the
/// cockpit's model picker via `GET /api/providers`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderDef {
    /// Stable provider id (the serde form, e.g. `claudecodev2`).
    pub id: &'static str,
    /// Human-readable provider name shown in the picker.
    pub name: &'static str,
    /// One-line provider blurb shown under the name.
    pub description: &'static str,
    /// The provider's selectable models.
    pub models: Vec<ModelDef>,
}

/// One selectable model, with pricing and context metadata for the picker.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelDef {
    /// Per-provider enum id (kebab-case, e.g. `claude-opus48`).
    pub id: &'static str,
    /// The provider API's canonical model name.
    pub api_name: &'static str,
    /// Human-readable model name shown in the picker.
    pub display_name: &'static str,
    /// Maximum context window in tokens.
    pub context_window: u64,
    /// Maximum output tokens per response.
    pub max_output: u64,
    /// Input price, USD per million tokens.
    pub input_price: f64,
    /// Output price, USD per million tokens.
    pub output_price: f64,
    /// Optional short badge (e.g. `Balanced`, `Fastest`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<&'static str>,
    /// Whether this is the provider's default model.
    pub is_default: bool,
}

// ── Registry ────────────────────────────────────────────────────────────

/// The full provider catalogue surfaced to the cockpit, in picker order.
fn all_providers() -> Vec<ProviderDef> {
    vec![
        provider_claudecodev2(),
        provider_anthropic(),
        provider_grok(),
        provider_groq(),
        provider_deepseek(),
        provider_minimax(),
    ]
}

/// The Claude Code OAuth provider and its model catalogue.
fn provider_claudecodev2() -> ProviderDef {
    let mut models = claudecodev2_opus_models();
    models.extend(claudecodev2_other_models());
    ProviderDef {
        id: "claudecodev2",
        name: "Claude Code",
        description: "OAuth \u{2014} Opus 5 \u{b7} 4.8 \u{b7} 4.6 \u{b7} Sonnet 5 \u{b7} Fable 5 \u{b7} Haiku 4.5",
        models,
    }
}

/// The Opus family for the Claude Code catalogue (5 / 4.8 / 4.6).
fn claudecodev2_opus_models() -> Vec<ModelDef> {
    vec![
        ModelDef {
            id: "claude-opus5",
            api_name: "claude-opus-5",
            display_name: "Opus 5",
            context_window: 200_000,
            max_output: 64_000,
            input_price: 5.0,
            output_price: 25.0,
            badge: Some("Most capable"),
            is_default: true,
        },
        ModelDef {
            id: "claude-opus48",
            api_name: "claude-opus-4-8",
            display_name: "Opus 4.8",
            context_window: 200_000,
            max_output: 64_000,
            input_price: 5.0,
            output_price: 25.0,
            badge: None,
            is_default: false,
        },
        ModelDef {
            id: "claude-opus46",
            api_name: "claude-opus-4-6",
            display_name: "Opus 4.6",
            context_window: 200_000,
            max_output: 64_000,
            input_price: 5.0,
            output_price: 25.0,
            badge: None,
            is_default: false,
        },
    ]
}

/// The Sonnet / Fable / Haiku tail of the Claude Code catalogue.
fn claudecodev2_other_models() -> Vec<ModelDef> {
    vec![
        ModelDef {
            id: "claude-sonnet5",
            api_name: "claude-sonnet-5",
            display_name: "Sonnet 5",
            context_window: 1_000_000,
            max_output: 64_000,
            input_price: 3.0,
            output_price: 15.0,
            badge: Some("Balanced"),
            is_default: false,
        },
        ModelDef {
            id: "claude-fable5",
            api_name: "claude-fable-5",
            display_name: "Fable 5",
            context_window: 400_000,
            max_output: 64_000,
            input_price: 10.0,
            output_price: 50.0,
            badge: Some("Creative"),
            is_default: false,
        },
        ModelDef {
            id: "claude-haiku45",
            api_name: "claude-haiku-4-5-20251001",
            display_name: "Haiku 4.5",
            context_window: 200_000,
            max_output: 64_000,
            input_price: 1.0,
            output_price: 5.0,
            badge: Some("Fast & cheap"),
            is_default: false,
        },
    ]
}

/// The direct Anthropic API-key provider and its model catalogue.
fn provider_anthropic() -> ProviderDef {
    ProviderDef {
        id: "anthropic",
        name: "Anthropic (API Key)",
        description: "Direct API \u{2014} Opus 4.5 \u{b7} Sonnet 4.5 \u{b7} Haiku 4.5",
        models: vec![
            ModelDef {
                id: "claude-opus45",
                api_name: "claude-opus-4-6",
                display_name: "Opus 4.5",
                context_window: 200_000,
                max_output: 128_000,
                input_price: 5.0,
                output_price: 25.0,
                badge: Some("Most capable"),
                is_default: true,
            },
            ModelDef {
                id: "claude-sonnet45",
                api_name: "claude-sonnet-4-5-20250929",
                display_name: "Sonnet 4.5",
                context_window: 200_000,
                max_output: 64_000,
                input_price: 3.0,
                output_price: 15.0,
                badge: Some("Balanced"),
                is_default: false,
            },
            ModelDef {
                id: "claude-haiku45",
                api_name: "claude-haiku-4-5-20251001",
                display_name: "Haiku 4.5",
                context_window: 200_000,
                max_output: 64_000,
                input_price: 1.0,
                output_price: 5.0,
                badge: Some("Fast & cheap"),
                is_default: false,
            },
        ],
    }
}

/// The xAI Grok provider and its model catalogue.
fn provider_grok() -> ProviderDef {
    ProviderDef {
        id: "grok",
        name: "xAI Grok",
        description: "Fast tool-calling \u{b7} 2M context",
        models: vec![
            ModelDef {
                id: "grok41-fast",
                api_name: "grok-4-1-fast",
                display_name: "Grok 4.1 Fast",
                context_window: 2_000_000,
                max_output: 128_000,
                input_price: 0.2,
                output_price: 0.5,
                badge: Some("Latest"),
                is_default: true,
            },
            ModelDef {
                id: "grok4-fast",
                api_name: "grok-4-fast",
                display_name: "Grok 4 Fast",
                context_window: 2_000_000,
                max_output: 128_000,
                input_price: 0.2,
                output_price: 0.5,
                badge: None,
                is_default: false,
            },
        ],
    }
}

/// The Groq provider and its model catalogue.
fn provider_groq() -> ProviderDef {
    ProviderDef {
        id: "groq",
        name: "Groq",
        description: "Ultra-fast inference \u{b7} GPT-OSS \u{b7} Llama",
        models: vec![
            ModelDef {
                id: "gpt-oss120b",
                api_name: "openai/gpt-oss-120b",
                display_name: "GPT-OSS 120B (+web)",
                context_window: 0x0002_0000,
                max_output: 128_000,
                input_price: 1.2,
                output_price: 1.2,
                badge: Some("Large"),
                is_default: true,
            },
            ModelDef {
                id: "gpt-oss20b",
                api_name: "openai/gpt-oss-20b",
                display_name: "GPT-OSS 20B (+web)",
                context_window: 0x0002_0000,
                max_output: 128_000,
                input_price: 0.2,
                output_price: 0.2,
                badge: None,
                is_default: false,
            },
            ModelDef {
                id: "llama33-70b",
                api_name: "llama-3.3-70b-versatile",
                display_name: "Llama 3.3 70B",
                context_window: 0x0002_0000,
                max_output: 128_000,
                input_price: 0.59,
                output_price: 0.79,
                badge: None,
                is_default: false,
            },
            ModelDef {
                id: "llama31-8b",
                api_name: "llama-3.1-8b-instant",
                display_name: "Llama 3.1 8B",
                context_window: 0x0002_0000,
                max_output: 128_000,
                input_price: 0.05,
                output_price: 0.08,
                badge: Some("Fastest"),
                is_default: false,
            },
        ],
    }
}

/// The DeepSeek provider and its model catalogue.
fn provider_deepseek() -> ProviderDef {
    ProviderDef {
        id: "deepseek",
        name: "DeepSeek",
        description: "V4 Flash & Pro \u{b7} 1M context",
        models: vec![
            ModelDef {
                id: "v4-flash",
                api_name: "deepseek-v4-flash",
                display_name: "V4 Flash",
                context_window: 1_000_000,
                max_output: 384_000,
                input_price: 0.14,
                output_price: 0.28,
                badge: Some("Cheap"),
                is_default: true,
            },
            ModelDef {
                id: "v4-pro",
                api_name: "deepseek-v4-pro",
                display_name: "V4 Pro",
                context_window: 1_000_000,
                max_output: 384_000,
                input_price: 0.435,
                output_price: 0.87,
                badge: Some("Capable"),
                is_default: false,
            },
        ],
    }
}

/// The `MiniMax` provider and its model catalogue.
fn provider_minimax() -> ProviderDef {
    ProviderDef {
        id: "minimax",
        name: "MiniMax",
        description: "M2.7 \u{2014} Anthropic-compatible API",
        models: vec![
            ModelDef {
                id: "m27",
                api_name: "MiniMax-M2.7",
                display_name: "M2.7",
                context_window: 204_800,
                max_output: 128_000,
                input_price: 2.0,
                output_price: 8.0,
                badge: None,
                is_default: true,
            },
            ModelDef {
                id: "m27-highspeed",
                api_name: "MiniMax-M2.7",
                display_name: "M2.7 Highspeed",
                context_window: 0x0002_0000,
                max_output: 128_000,
                input_price: 4.0,
                output_price: 16.0,
                badge: Some("Fast"),
                is_default: false,
            },
        ],
    }
}

// ── Handler ─────────────────────────────────────────────────────────────

/// `GET /api/providers` — the LLM provider + model registry, restricted to the
/// providers whose API key is configured (so the cockpit only ever offers
/// models that can actually run). Each model carries a server-built `key`
/// (`"<providerId>:<modelId>"`) — the canonical id the org allowlist is keyed
/// on, so the frontend never has to build it.
///
/// With `?allowed=1` the org model allowlist is applied as well: models outside
/// the allowlist are dropped and providers left with no model are omitted (an
/// **empty** allowlist means *all allowed*). The per-agent model picker uses
/// this variant; the admin allowlist editor omits it so it can see — and toggle
/// — every usable model.
pub(crate) fn providers(query: &str) -> HttpReply {
    let apply_allowlist = has_param(query, "allowed");
    let allowlist = if apply_allowlist { rest::allowed_models() } else { Vec::new() };
    let allow_set: std::collections::HashSet<&str> = allowlist.iter().map(String::as_str).collect();
    // An empty allowlist (the delivery default) means "everything allowed".
    let restrict = apply_allowlist && !allow_set.is_empty();

    // What the gateway declares it can route, fetched once for the whole reply
    // (cached across replies). `None` = no gateway, or it could not be asked; the
    // catalogue then passes through unfiltered.
    let declared = gateway_models::declared();

    let mut out: Vec<serde_json::Value> = Vec::new();
    for p in all_providers() {
        // Usable = its credential is configured (API key, or — for the Claude
        // Code OAuth backends — a present, non-expired credentials file). Drop
        // the rest server-side.
        if !provider_usable(p.id) {
            continue;
        }
        // Under a gateway, this provider's models are only offerable if the proxy
        // declares them — see `gateway_offers`.
        let gated = cp_base::config::llm_gateway::is_active()
            && cp_base::config::llm_gateway::declared_in_model_list(p.id);
        // Annotate each model with its canonical "<provider>:<model>" key,
        // applying the org allowlist when this is the picker variant.
        let models: Vec<serde_json::Value> = p
            .models
            .iter()
            .filter_map(|m| {
                let key = format!("{}:{}", p.id, m.id);
                if restrict && !allow_set.contains(key.as_str()) {
                    return None;
                }
                if !gateway_offers(gated, declared.as_ref(), m.api_name) {
                    return None;
                }
                let mut mv = serde_json::to_value(m).unwrap_or_default();
                if let Some(obj) = mv.as_object_mut() {
                    drop(obj.insert("key".to_owned(), serde_json::Value::String(key)));
                }
                Some(mv)
            })
            .collect();
        // Never surface a provider the allowlist emptied out.
        if models.is_empty() {
            continue;
        }
        out.push(serde_json::json!({
            "id": p.id,
            "name": p.name,
            "description": p.description,
            "models": models,
        }));
    }
    match serde_json::to_string(&out) {
        Ok(body) => HttpReply { status: 200, body },
        Err(e) => HttpReply::error(500, &format!("serialize: {e}")),
    }
}

/// May `api_name` be offered, given what the gateway declares?
///
/// `gated` is false for a provider the gateway does not carry, or when there is
/// no gateway — the catalogue then decides alone. When `declared` is `None` the
/// answer is yes as well: not knowing what the proxy routes is a reason to keep
/// the catalogue, not to empty the picker (a proxy that is briefly unreachable
/// would otherwise look like a product that lost its models).
fn gateway_offers(gated: bool, declared: Option<&std::collections::HashSet<String>>, api_name: &str) -> bool {
    !gated || declared.is_none_or(|set| set.contains(api_name))
}

/// Is `key` present as a query parameter (with or without a value)?
fn has_param(query: &str, key: &str) -> bool {
    query.split('&').filter(|s| !s.is_empty()).any(|pair| pair.split_once('=').map_or(pair, |(k, _)| k) == key)
}

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
/// restart.
/// Under a gateway (`CP_LLM_GATEWAY`) the providers it carries are usable with no
/// local key at all: the credential that reaches the provider is the one held by
/// the proxy, and this process is not supposed to have a copy. Without this the
/// cockpit offers nothing on a gateway deployment — an empty model picker with no
/// error anywhere, which is not a diagnosable failure.
fn provider_usable(id: &str) -> bool {
    if cp_base::config::llm_gateway::is_active() && cp_base::config::llm_gateway::routes_provider(id) {
        return true;
    }
    match id {
        "claudecodev2" => oauth_creds::claude_oauth_available(),
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

/// Resolve a model's public `apiName` from its provider id + enum id.
///
/// `config.json` stores the agent's current model as the per-provider enum id
/// (kebab-case, e.g. `claude-opus48`), whereas the web picker resolves a
/// selection by `apiName` (e.g. `claude-opus-4-8`). This bridges the two so the
/// shaped agent DTO can advertise a model the frontend can match. Returns
/// `None` when the pair is unknown.
pub(crate) fn resolve_api_name(provider_id: &str, model_id: &str) -> Option<&'static str> {
    all_providers()
        .into_iter()
        .find(|p| p.id == provider_id)?
        .models
        .into_iter()
        .find(|m| m.id == model_id)
        .map(|m| m.api_name)
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
