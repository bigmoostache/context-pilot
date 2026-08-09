//! Bridge command application for the agent's **own configuration** — its LLM
//! provider/model, its active behaviour agent, its self-identity, and its
//! public profile.
//!
//! Split from the sibling [`super`] module (which had outgrown the 500-line
//! limit) along the natural seam: the parent applies commands that mutate
//! *threads* (the conversation surface), while this file applies the ones that
//! mutate *the agent itself*.
//!
//! Every handler here shares one shape. It converts the wire payload into the
//! owning module's own type, delegates to that module's **shared core** — the
//! same core the equivalent local tool calls, so a web command and a tool
//! enforce identical validation — and logs a rejection rather than propagating
//! it, leaving the prior value untouched (all-or-nothing).

use cp_base::config::llm_types::LlmProvider;
use cp_base::config::models::{AnthropicModel, ClaudeCodeV2Model, DeepSeekModel, GrokModel, GroqModel, MiniMaxModel};
use cp_base::state::runtime::State;

// ── Configure (LLM provider + model) ───────────────────────────────────

/// Apply a provider+model change from the web frontend.
///
/// Both strings use the serde names from [`LlmProvider`] (lowercase) and
/// the per-provider model enums (kebab-case). Invalid names are logged and
/// ignored — the agent keeps its current config.
pub(super) fn apply_configure(state: &mut State, provider_str: &str, model_str: &str) {
    let provider_val = serde_json::Value::String(provider_str.to_owned());
    let Ok(provider) = serde_json::from_value::<LlmProvider>(provider_val) else {
        log::warn!("bridge: Configure unknown provider \"{provider_str}\"");
        return;
    };

    let model_val = serde_json::Value::String(model_str.to_owned());
    let model_ok = match provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCodeApiKey => {
            serde_json::from_value::<AnthropicModel>(model_val).map(|m| state.anthropic_model = m).is_ok()
        }
        LlmProvider::ClaudeCodeV2 => {
            serde_json::from_value::<ClaudeCodeV2Model>(model_val).map(|m| state.claude_code_v2_model = m).is_ok()
        }
        LlmProvider::Grok => serde_json::from_value::<GrokModel>(model_val).map(|m| state.grok_model = m).is_ok(),
        LlmProvider::Groq => serde_json::from_value::<GroqModel>(model_val).map(|m| state.groq_model = m).is_ok(),
        LlmProvider::DeepSeek => {
            serde_json::from_value::<DeepSeekModel>(model_val).map(|m| state.deepseek_model = m).is_ok()
        }
        LlmProvider::MiniMax => {
            serde_json::from_value::<MiniMaxModel>(model_val).map(|m| state.minimax_model = m).is_ok()
        }
    };

    if !model_ok {
        log::warn!("bridge: Configure unknown model \"{model_str}\" for provider \"{provider_str}\"");
        return;
    }

    state.llm_provider = provider;
    state.flags.ui.dirty = true;
    log::info!("bridge: configured provider={provider_str} model={model_str}");
}

// ── LoadBehaviour (active behaviour agent) ─────────────────────────────

/// Switch the agent's active behaviour agent (prompt-library system prompt)
/// from the web footer selector. An empty `id` reverts to the default agent.
///
/// Routed through the shared [`cp_mod_prompt::tools::set_active_agent`] so the
/// bridge command and the local `agent_load` tool mutate the same
/// `PromptState.active_agent_id` through one path (no duplication). The touched
/// SYSTEM + LIBRARY panels re-render; the active flag surfaces to the web
/// footer on the next `library()` inspect read.
pub(super) fn apply_load_behaviour(state: &mut State, id: &str) {
    match cp_mod_prompt::tools::set_active_agent(state, id) {
        Ok(name) => {
            state.flags.ui.dirty = true;
            log::info!("bridge: loaded behaviour agent {name} (id={id:?})");
        }
        Err(e) => log::warn!("bridge: LoadBehaviour failed: {e}"),
    }
}

// ── SetIdentity (self-identity) ────────────────────────────────────────

/// Apply a self-identity change from the web agent-settings form.
///
/// Maps the wire [`cp_wire::types::command::SelfIdentity`] value object onto the
/// Agora module's own `SelfIdentity` and writes it through the shared
/// [`cp_agora::tools::set_identity`] core — the exact validated path the local
/// `Agora_set_identity` tool uses, so both surfaces enforce the same per-value
/// word cap (all-or-nothing on overflow) and touch the same AGORA panel. A
/// rejected overflow leaves the prior identity untouched and is logged.
pub(super) fn apply_set_identity(state: &mut State, spec: cp_wire::types::command::SelfIdentity) {
    let next = cp_agora::types::SelfIdentity {
        identity: spec.identity,
        values: spec.values,
        principles: spec.principles,
        character: spec.character,
        expertise: spec.expertise,
        role: spec.role,
        operational_responsibilities: spec.operational_responsibilities,
        knowledge_responsibilities: spec.knowledge_responsibilities,
        organic_responsibilities: spec.organic_responsibilities,
        direct_management: spec.direct_management,
    };
    match cp_agora::tools::set_identity(state, next) {
        Ok(()) => {
            state.flags.ui.dirty = true;
            log::info!("bridge: applied SetIdentity");
        }
        Err(e) => log::warn!("bridge: SetIdentity rejected: {e}"),
    }
}

// ── SetProfile (display slug + image reference) ────────────────────────

/// Apply a profile change — the agent's display slug and/or image reference —
/// arriving from the backend (a dashboard rename, an avatar upload, or an
/// avatar removal).
///
/// Both fields are **patch-shaped**: `None` leaves the current value untouched.
/// That is what lets a rename and an avatar change ride the same command
/// without either clobbering the other, and the semantics are enforced once, in
/// the shared [`cp_agora::tools::set_profile`] core, rather than at each caller.
///
/// The core validates before it writes (slug single-line and length-capped;
/// image either an `http(s)` URL or a realm-relative path that cannot escape
/// the realm), so a rejected value leaves the prior profile untouched and is
/// logged — the same all-or-nothing contract as [`apply_set_identity`].
///
/// Nothing here re-advertises: the registry record is a projection
/// ([`emit_advert`](crate::app::run::threads::emit_advert)), so the next
/// main-loop tick observes the changed state and republishes on its own. That
/// is precisely why the ownership move is safe — a new mutation site cannot
/// forget to publish.
pub(super) fn apply_set_profile(state: &mut State, slug: Option<String>, image: Option<String>) {
    match cp_agora::tools::set_profile(state, slug, image) {
        Ok(()) => {
            state.flags.ui.dirty = true;
            log::info!("bridge: applied SetProfile");
        }
        Err(e) => log::warn!("bridge: SetProfile rejected: {e}"),
    }
}
