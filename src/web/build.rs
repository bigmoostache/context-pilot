//! `build_web_state()` — the web mirror of `build_frame()`.
//!
//! Assembles the `WebState` view-model (see `docs/nestor-web-contract.md`)
//! from the domain `State`. Pure read: no mutation, no transport. Each
//! section has its own builder so the sink can hash and diff them
//! independently (snapshot + deltas).

use cp_base::state::data::model_helpers::ModelPricing as _;
use serde_json::{Value, json};

use crate::state::{Kind, State};

/// Build the `status` section (stream phase, config, token accounting).
pub(crate) fn status_value(state: &State) -> Value {
    let spine = cp_mod_spine::types::SpineState::get(state);
    let think_threshold = state.get_ext::<crate::modules::questions::ThinkState>().map(|ts| ts.reminder_threshold);
    let phase = match state.flags.stream.phase {
        cp_base::state::flags::StreamPhase::Idle => "idle",
        cp_base::state::flags::StreamPhase::Receiving => "receiving",
        cp_base::state::flags::StreamPhase::ExecutingTools => "executing_tools",
    };
    let context_used: usize = state.context.iter().map(|c| c.token_count).fold(0, usize::saturating_add);
    json!({
        "stream_phase": phase,
        "streaming_tool": state.streaming_tool.as_ref().map(|st| json!({
            "name": st.name, "input_so_far": st.input_so_far,
        })),
        "guard_rail_blocked": state.guard_rail_blocked,
        "last_stop_reason": state.last_stop_reason,
        "api_check_in_progress": state.flags.lifecycle.api_check_in_progress,
        "api_check": state.api_check_result.as_ref().map(|res| json!({
            "ok": res.all_ok(), "auth_ok": res.auth_ok, "streaming_ok": res.streaming_ok,
            "tools_ok": res.tools_ok, "error": res.error,
        })),
        "provider": state.llm_provider,
        "model": state.current_model(),
        "secondary_provider": state.secondary_provider,
        "secondary_model": secondary_model_id(state),
        "theme": state.active_theme,
        "auto_continue": spine.config.continue_until_todos_done,
        "reverie_enabled": state.flags.config.reverie_enabled,
        "think_threshold": think_threshold,
        "max_cost": spine.config.max_cost,
        "cleaning_threshold": state.cleaning_threshold,
        "cleaning_target": state.cleaning_target_proportion,
        "context_used_tokens": context_used,
        "context_budget": state.context_budget,
        "context_window": state.model_context_window(),
        "session_tokens": {
            "cache_hit": state.cache_hit_tokens, "cache_miss": state.cache_miss_tokens,
            "output": state.total_output_tokens, "uncached_input": state.uncached_input_tokens,
        },
        "tick_tokens": {
            "cache_hit": state.tick_cache_hit_tokens, "cache_miss": state.tick_cache_miss_tokens,
            "output": state.tick_output_tokens, "uncached_input": state.tick_uncached_input_tokens,
        },
        "alive_breakpoints": state.tick_alive_breakpoints,
        "bp_positions_permille": state.tick_alive_bp_positions,
        "spine_notifications": spine.notifications.iter().filter(|n| !n.is_processed()).count(),
    })
}

/// Serde ID of the secondary model for the secondary provider.
fn secondary_model_id(state: &State) -> Value {
    use cp_base::config::llm_types::LlmProvider;
    match state.secondary_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
            json!(state.secondary_anthropic_model)
        }
        LlmProvider::Grok => json!(state.secondary_grok_model),
        LlmProvider::Groq => json!(state.secondary_groq_model),
        LlmProvider::DeepSeek => json!(state.secondary_deepseek_model),
        LlmProvider::MiniMax => json!(state.secondary_minimax_model),
    }
}

/// Build the `panels` section: the sidebar list, sorted by numeric ID.
pub(crate) fn panels_value(state: &State) -> Value {
    let mut indices: Vec<usize> = (0..state.context.len()).collect();
    indices.sort_by_key(|&idx| {
        state
            .context
            .get(idx)
            .and_then(|c| c.id.strip_prefix('P'))
            .and_then(|num| num.parse::<usize>().ok())
            .unwrap_or(usize::MAX)
    });
    let panels: Vec<Value> = indices
        .iter()
        .filter_map(|&idx| state.context.get(idx).map(|ctx| panel_entry(ctx, idx == state.selected_context)))
        .collect();
    json!(panels)
}

/// One sidebar entry.
fn panel_entry(ctx: &cp_base::state::context::Entry, selected: bool) -> Value {
    json!({
        "id": ctx.id, "uid": ctx.uid, "kind": ctx.context_type.as_str(),
        "name": ctx.name, "is_fixed": ctx.context_type.is_fixed(), "selected": selected,
        "token_count": ctx.token_count, "full_token_count": ctx.full_token_count,
        "page": ctx.current_page, "total_pages": ctx.total_pages,
        "last_refresh_ms": ctx.last_refresh_ms,
    })
}

/// Build the `active_panel` section: content of the selected panel.
/// The conversation panel returns `null` content — messages live in the
/// `conversation` section.
pub(crate) fn active_panel_value(state: &State) -> Value {
    let Some(ctx) = state.context.get(state.selected_context) else { return Value::Null };
    panel_content_value(ctx)
}

/// Content payload for any panel (used by `active_panel` and the
/// `panel_content` query).
pub(crate) fn panel_content_value(ctx: &cp_base::state::context::Entry) -> Value {
    let content = if ctx.context_type.as_str() == Kind::CONVERSATION {
        Value::Null
    } else {
        json!(ctx.cached_content.clone().unwrap_or_default())
    };
    json!({
        "id": ctx.id, "kind": ctx.context_type.as_str(), "name": ctx.name,
        "content": content, "metadata": ctx.metadata,
    })
}

/// Build the `question_form` section (`null` when none pending).
pub(crate) fn question_form_value(state: &State) -> Value {
    let Some(form) = state.get_ext::<cp_base::ui::question_form::PendingForm>() else { return Value::Null };
    if form.resolved {
        return Value::Null;
    }
    let questions: Vec<Value> = form
        .questions
        .iter()
        .map(|question| {
            json!({
                "text": question.text, "header": question.header, "multi_select": question.multi_select,
                "options": question.options.iter().map(|opt| json!({
                    "label": opt.label, "description": opt.description,
                })).collect::<Vec<Value>>(),
            })
        })
        .collect();
    json!({ "tool_use_id": form.tool_use_id, "questions": questions })
}

/// Build one conversation message.
pub(crate) fn message_value(msg: &cp_base::state::data::message::Message) -> Value {
    use cp_base::state::data::message::{MsgKind, MsgStatus};
    let kind = match msg.msg_type {
        MsgKind::TextMessage => "text",
        MsgKind::ToolCall => "tool_call",
        MsgKind::ToolResult => "tool_result",
    };
    let status = match msg.status {
        MsgStatus::Full => "full",
        MsgStatus::Deleted => "deleted",
        MsgStatus::Detached => "detached",
    };
    json!({
        "id": msg.id, "uid": msg.uid, "role": msg.role, "kind": kind,
        "content": msg.content, "status": status,
        "tool_uses": msg.tool_uses.iter().map(|tu| json!({
            "id": tu.id, "name": tu.name, "input": tu.input,
        })).collect::<Vec<Value>>(),
        "tool_results": msg.tool_results.iter().map(|tr| json!({
            "tool_use_id": tr.tool_use_id,
            "content": tr.display.clone().unwrap_or_else(|| tr.content.clone()),
            "tldr": tr.tldr, "is_error": tr.is_error, "tool_name": tr.tool_name,
        })).collect::<Vec<Value>>(),
        "timestamp_ms": msg.timestamp_ms,
    })
}

/// Cheap per-message fingerprint for delta detection (no serialization).
pub(crate) fn message_fingerprint(msg: &cp_base::state::data::message::Message) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    msg.content.hash(&mut hasher);
    msg.tool_uses.len().hash(&mut hasher);
    msg.tool_results.len().hash(&mut hasher);
    for result in &msg.tool_results {
        result.content.hash(&mut hasher);
        result.is_error.hash(&mut hasher);
    }
    let status_tag: u8 = match msg.status {
        cp_base::state::data::message::MsgStatus::Full => 0,
        cp_base::state::data::message::MsgStatus::Deleted => 1,
        cp_base::state::data::message::MsgStatus::Detached => 2,
    };
    status_tag.hash(&mut hasher);
    msg.input_tokens.hash(&mut hasher);
    hasher.finish()
}

/// Build the static `meta` section (sent with snapshots only).
pub(crate) fn meta_value(state: &State) -> Value {
    let tools: Vec<Value> = state
        .tools
        .iter()
        .map(|tool| {
            json!({
                "id": tool.id, "name": tool.name, "short_desc": tool.short_desc, "enabled": tool.enabled,
            })
        })
        .collect();
    let workspace = std::env::current_dir().map_or_else(|_e| String::new(), |dir| dir.display().to_string());
    json!({
        "themes": cp_base::config::THEME_ORDER,
        "providers": providers_catalog(),
        "tools": tools,
        "workspace": workspace,
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// Serialize one model variant as `{id, label}`.
fn model_entry<M: serde::Serialize + cp_base::config::llm_types::ModelInfo>(variant: &M) -> Value {
    json!({ "id": variant, "label": variant.display_name() })
}

/// The Anthropic roster, shared by the three Anthropic-backed providers.
fn anthropic_roster() -> Value {
    use cp_base::config::llm_types::AnthropicModel;
    json!([
        model_entry(&AnthropicModel::ClaudeOpus45),
        model_entry(&AnthropicModel::ClaudeSonnet45),
        model_entry(&AnthropicModel::ClaudeHaiku45),
    ])
}

/// Static provider/model catalog — mirrors the TUI config overlay roster.
fn providers_catalog() -> Value {
    use cp_base::config::llm_types::{DeepSeekModel, GrokModel, GroqModel, MiniMaxModel};
    json!([
        { "id": "anthropic", "label": "Anthropic", "models": anthropic_roster() },
        { "id": "claudecode", "label": "Claude Code (OAuth)", "models": anthropic_roster() },
        { "id": "claudecodeapikey", "label": "Claude Code (API key)", "models": anthropic_roster() },
        { "id": "grok", "label": "Grok", "models": [
            model_entry(&GrokModel::Grok41Fast), model_entry(&GrokModel::Grok4Fast),
        ]},
        { "id": "groq", "label": "Groq", "models": [
            model_entry(&GroqModel::GptOss120b), model_entry(&GroqModel::GptOss20b),
            model_entry(&GroqModel::Llama33_70b), model_entry(&GroqModel::Llama31_8b),
        ]},
        { "id": "deepseek", "label": "DeepSeek", "models": [
            model_entry(&DeepSeekModel::V4Flash), model_entry(&DeepSeekModel::V4Pro),
        ]},
        { "id": "minimax", "label": "MiniMax", "models": [
            model_entry(&MiniMaxModel::M27), model_entry(&MiniMaxModel::M27Highspeed),
        ]},
    ])
}

/// Serialize the complete snapshot frame for one connection.
pub(crate) fn snapshot_json(state: &State) -> String {
    let conversation: Vec<Value> = state.messages.iter().map(message_value).collect();
    json!({
        "t": "snapshot",
        "state": {
            "status": status_value(state),
            "panels": panels_value(state),
            "active_panel": active_panel_value(state),
            "conversation": conversation,
            "question_form": question_form_value(state),
            "input_draft": state.input,
            "meta": meta_value(state),
        },
    })
    .to_string()
}
