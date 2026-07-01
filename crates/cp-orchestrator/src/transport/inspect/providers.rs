//! LLM provider + model registry — the single source of truth for all provider
//! and model metadata surfaced to the frontend (model picker, config panel,
//! per-agent manage modal).
//!
//! The registry mirrors the TUI's `LlmProvider` / `ModelInfo` enums. Keeping it
//! here (rather than in `cp-wire`) avoids coupling the shared protocol crate to
//! pricing/display metadata that only the orchestrator and frontend need.
//!
//! The frontend imports `ProviderDef` and `ModelDef` from the generated OpenAPI
//! client and fetches the data via `GET /api/providers` — zero hardcoded model
//! lists in TypeScript.

use serde::Serialize;

use crate::transport::rest::HttpReply;

// ── Wire types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub models: Vec<ModelDef>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelDef {
    pub id: &'static str,
    pub api_name: &'static str,
    pub display_name: &'static str,
    pub context_window: u64,
    pub max_output: u64,
    pub input_price: f64,
    pub output_price: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<&'static str>,
    #[serde(skip_serializing_if = "is_false")]
    pub is_default: bool,
}

fn is_false(v: &bool) -> bool {
    !v
}

// ── Registry ────────────────────────────────────────────────────────────

fn all_providers() -> Vec<ProviderDef> {
    vec![
        ProviderDef {
            id: "claudecodev2",
            name: "Claude Code V2",
            description: "OAuth — Opus 4.8 · Fable 5 · Sonnet 4.6",
            models: vec![
                ModelDef {
                    id: "claude-opus48",
                    api_name: "claude-opus-4-8",
                    display_name: "Opus 4.8",
                    context_window: 200_000,
                    max_output: 64_000,
                    input_price: 5.0,
                    output_price: 25.0,
                    badge: Some("Most capable"),
                    is_default: true,
                },
                ModelDef {
                    id: "claude-sonnet46",
                    api_name: "claude-sonnet-4-6",
                    display_name: "Sonnet 4.6",
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
            ],
        },
        ProviderDef {
            id: "anthropic",
            name: "Anthropic",
            description: "Direct API — Opus 4.5 · Sonnet 4.5 · Haiku 4.5",
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
        },
        ProviderDef {
            id: "claudecode",
            name: "Claude Code (OAuth)",
            description: "OAuth V1 — Opus 4.5 · Sonnet 4.5 · Haiku 4.5",
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
        },
        ProviderDef {
            id: "claudecodeapikey",
            name: "Claude Code (API Key)",
            description: "API key — Opus 4.5 · Sonnet 4.5 · Haiku 4.5",
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
        },
        ProviderDef {
            id: "grok",
            name: "xAI Grok",
            description: "Fast tool-calling · 2M context",
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
        },
        ProviderDef {
            id: "groq",
            name: "Groq",
            description: "Ultra-fast inference · GPT-OSS · Llama",
            models: vec![
                ModelDef {
                    id: "gpt-oss120b",
                    api_name: "openai/gpt-oss-120b",
                    display_name: "GPT-OSS 120B (+web)",
                    context_window: 131_072,
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
                    context_window: 131_072,
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
                    context_window: 131_072,
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
                    context_window: 131_072,
                    max_output: 128_000,
                    input_price: 0.05,
                    output_price: 0.08,
                    badge: Some("Fastest"),
                    is_default: false,
                },
            ],
        },
        ProviderDef {
            id: "deepseek",
            name: "DeepSeek",
            description: "V4 Flash & Pro · 1M context",
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
        },
        ProviderDef {
            id: "minimax",
            name: "MiniMax",
            description: "M2.7 — Anthropic-compatible API",
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
                    context_window: 131_072,
                    max_output: 128_000,
                    input_price: 4.0,
                    output_price: 16.0,
                    badge: Some("Fast"),
                    is_default: false,
                },
            ],
        },
    ]
}

// ── Handler ─────────────────────────────────────────────────────────────

/// `GET /api/providers` — returns the full LLM provider + model registry.
pub(crate) fn providers() -> HttpReply {
    let data = all_providers();
    match serde_json::to_string(&data) {
        Ok(body) => HttpReply { status: 200, body },
        Err(e) => HttpReply::error(500, &format!("serialize: {e}")),
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
