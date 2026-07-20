use cp_base::state::data::model_helpers::ModelPricing as _;

use crate::app::panels::{ContextItem, collect_all_context, refresh_all_panels};
use crate::infra::tools::ToolDefinition;
use crate::infra::tools::refresh_conversation_context;
use crate::modules;
use crate::state::{Message, State};

mod detach;
/// Freeze policy: per-panel and ordering freeze decisions (queue, tempo, breath budget).
mod freeze;
use freeze::freeze_conditions;
/// Per-panel freeze pass (normal path): hash, decide freeze/fresh, cost + telemetry.
mod freeze_pass;
use freeze_pass::FreezeMeta;

/// Context data prepared for streaming
pub(super) struct StreamContext {
    /// Filtered conversation messages for the LLM.
    pub messages: Vec<Message>,
    /// Collected and sorted context items from all panels.
    pub context_items: Vec<ContextItem>,
    /// Tool definitions available for this streaming session.
    pub tools: Vec<ToolDefinition>,
}

/// Optional reverie context: when provided, replaces the conversation section
/// with P-main-conv (main AI's conversation as a read-only panel) and the
/// reverie's own messages. Panels and tools remain IDENTICAL for cache hits.
pub(super) struct ReverieContext {
    /// Agent ID driving this reverie (e.g., "cleaner", "cartographer")
    pub agent_id: String,
    /// The reverie's own conversation messages (may be empty on first run)
    pub messages: Vec<Message>,
    /// Tool restrictions preamble injected at the top of the reverie conversation
    pub tool_restrictions: String,
}

/// Refresh all context elements and prepare data for streaming.
///
/// Every call to this function means the LLM is about to see the full
/// conversation history (including any user messages that arrived during
/// streaming). We therefore mark all `UserMessage` notifications as processed
/// here — the LLM has "seen" them via the rebuilt context.
pub(super) fn prepare_stream_context(
    state: &mut State,
    include_last_message: bool,
    reverie: Option<ReverieContext>,
) -> StreamContext {
    let _fg = cp_base::flame!("prepare_context");
    // Mark UserMessage notifications as processed on every context rebuild.
    // This prevents the spine from firing a redundant auto-continuation for
    // messages the LLM already saw (e.g., user sent a message during a tool
    // call pause — the message is in context, LLM responds, but without this
    // the notification would still be "unprocessed" when the stream ends).
    cp_mod_spine::types::SpineState::mark_all_unprocessed_as_processed(state);

    // Compute freeze conditions early — needed to guard detachment below
    // and reused later for the unified freeze pass.
    let cond = freeze_conditions(state);

    // Detach old conversation chunks — but NOT when the freeze engine is
    // preserving the prompt prefix. Detaching creates new panels and drains
    // messages, which would break the exact cache prefix that freezing protects.
    if !cond.freeze_order() {
        detach::detach_conversation_chunks(state);
    }

    // Refresh conversation token counts (not panel-based yet)
    refresh_conversation_context(state);

    // Refresh all panel token counts
    refresh_all_panels(state);

    // Collect all context items from panels
    let mut context_items = collect_all_context(state);

    // === Panel ordering ===
    // When frozen, panels keep their previous sorted positions — no reordering
    // from `last_refresh_ms` changes. When unfrozen, sort by freshness and save.
    // (`cond` was computed early — before detach guard — and is still valid here:
    // neither queue_active nor tempo changed between there and here.)

    // === Tempo lifecycle ===
    // Read the current tempo flag for this tick's freeze decisions, then reset.
    // If tempo is true (no tool broke it last tick), we freeze everything.
    // (cond already captured state.tempo above)
    state.tempo = true; // Reset for next tick — tools will break it if they execute

    if cond.freeze_order() && !state.previous_panel_order.is_empty() {
        // Reorder context_items to match the saved order, dropping unknowns
        let order = &state.previous_panel_order;
        context_items.sort_by_key(|item| order.iter().position(|id| *id == item.id).unwrap_or(usize::MAX));
        context_items.retain(|item| order.contains(&item.id));
    } else {
        context_items.sort_by_key(|item| item.last_refresh_ms);
        state.previous_panel_order = context_items.iter().map(|item| item.id.clone()).collect();
    }

    // === Unified freeze pass (queue + breath + cost tracking) ═══════════════
    //
    // Two paths: full-freeze (snapshot replay) or normal (per-panel decisions).
    //
    // Full-freeze activates when the freeze engine is active (queue OR tempo)
    // AND a snapshot from a previous unfrozen tick exists. It replays the exact
    // panel content from that snapshot, eliminating cache breaks from panel
    // disappearance, appearance, or missing emitted snapshots.
    //
    // Normal path: single pass over all panels — hash, detect changes, consult
    // freeze policy, apply decision, snapshot what was emitted, track cost.

    // Pre-compute system + tools token prefix for tick telemetry
    let system_tokens = crate::state::estimate_tokens(&get_active_agent_content(state));
    let tools_tokens = modules::overview::context::estimate_tool_definitions_tokens(state);
    let prompt_prefix_tokens = system_tokens.saturating_add(tools_tokens);

    let full_freeze = cond.freeze_order() && state.frozen_context_snapshot.is_some();
    let meta = FreezeMeta { cond, prompt_prefix_tokens };

    if let (true, Some(snapshot)) = (full_freeze, state.frozen_context_snapshot.as_ref()) {
        // ═══ FULL FREEZE: replay exact previous prompt ═══════════════════════
        let snap = snapshot.clone();
        freeze_pass::apply_full_freeze(state, &mut context_items, &snap, meta);
    } else {
        // ═══ Normal path: per-panel freeze decisions ═════════════════════════
        freeze_pass::run_panel_freeze_pass(state, &mut context_items, meta);

        // Save snapshot for next frozen tick (panels only, no "chat")
        state.frozen_context_snapshot = Some(context_items.iter().filter(|i| i.id != "chat").cloned().collect());
    }

    // Check if context has breached the threshold — may activate the reverie optimizer
    let _r = crate::app::reverie::trigger::check_threshold_trigger(state);

    // Dynamically enable/disable panel_goto_page based on whether any panel is paginated
    let has_paginated = state.context.iter().any(|c| c.total_pages > 1);
    for tool in &mut state.tools {
        if tool.id == "panel_goto_page" {
            tool.enabled = has_paginated;
        }
    }

    // Prepare messages — branch based on whether this is a reverie or main worker
    if let Some(rev) = reverie {
        build_reverie_stream_context(state, context_items, rev)
    } else {
        build_main_stream_context(state, context_items, include_last_message)
    }
}

/// Assemble a reverie sub-agent's `StreamContext`: append the main agent's
/// conversation as a read-only `P-main-conv` panel and the reverie's own
/// agent-prompt + context + tool-restrictions `P-reverie` panel, then use the
/// reverie's messages as the conversation. Tools stay identical for cache hits.
fn build_reverie_stream_context(
    state: &State,
    mut context_items: Vec<ContextItem>,
    rev: ReverieContext,
) -> StreamContext {
    // Add P-main-conv: the main worker's conversation as a read-only panel
    let main_conv_content = cp_base::state::data::message::format_messages_to_chunk(
        &state
            .messages
            .iter()
            .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .cloned()
            .collect::<Vec<_>>(),
    );
    context_items.push(ContextItem::new(
        "P-main-conv",
        "Main Agent Conversation (read-only)",
        main_conv_content,
        crate::app::panels::now_ms(),
    ));

    context_items.push(ContextItem::new(
        "P-reverie",
        "Reverie Context (tool restrictions + conversation)",
        build_reverie_panel_content(state, &rev),
        crate::app::panels::now_ms(),
    ));

    // The reverie's messages ARE the conversation (may be empty on first run).
    // Tools are IDENTICAL to the main worker for prompt cache hits.
    // Report is described in the P-reverie panel text, not in the API tool list.
    StreamContext { messages: rev.messages, context_items, tools: state.tools.clone() }
}

/// Build the `P-reverie` panel body: agent instructions (injected here, NOT as
/// system prompt, to preserve cache hits) + additional context + tool
/// restrictions + a conversation-follows marker.
fn build_reverie_panel_content(state: &State, rev: &ReverieContext) -> String {
    let mut content = String::new();
    let agents = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Agent);
    if let Some(agent) = agents.iter().find(|a| a.id == rev.agent_id) {
        content.push_str("## Agent Instructions\n");
        content.push_str(&agent.content);
        content.push('\n');
    }
    if let Some(rev_state) = state.reveries.get(&rev.agent_id)
        && let Some(ctx) = rev_state.context.as_ref()
    {
        content.push_str("\n## Additional Context\n");
        content.push_str(ctx);
        content.push('\n');
    }
    content.push_str(&rev.tool_restrictions);
    if !rev.messages.is_empty() {
        content.push_str("\n## Reverie Conversation\n(Your messages follow in the conversation below)\n");
    }
    content
}

/// Assemble the main worker's `StreamContext`: filter empty messages, optionally
/// dropping the last (uncommitted) one.
fn build_main_stream_context(
    state: &State,
    context_items: Vec<ContextItem>,
    include_last_message: bool,
) -> StreamContext {
    let non_empty = state
        .messages
        .iter()
        .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty());
    let messages: Vec<_> = if include_last_message {
        non_empty.cloned().collect()
    } else {
        non_empty.take(state.messages.len().saturating_sub(1)).cloned().collect()
    };
    StreamContext { messages, context_items, tools: state.tools.clone() }
}

/// Build `StreamParams` from the current state and a `StreamContext`.
///
/// This is the **single canonical constructor** for streaming parameters. Both the main
/// worker and reverie sub-agents MUST use this function. It locks the shared prompt prefix
/// (provider, model, `max_output_tokens`, `system_prompt`) to the active worker config,
/// making it structurally impossible for the two paths to drift apart. The ONLY divergence
/// point is `seed_content` — the main worker re-injects its system prompt, while the
/// reverie injects its agent instructions + tool restrictions.
pub(crate) fn build_stream_params(
    state: &State,
    ctx: StreamContext,
    seed_content: Option<String>,
) -> crate::infra::api::StreamParams {
    let system_prompt = get_active_agent_content(state);
    crate::infra::api::StreamParams {
        provider: state.llm_provider,
        model: state.current_model(),
        max_output_tokens: state.current_max_output_tokens(),
        messages: ctx.messages,
        context_items: ctx.context_items,
        tools: ctx.tools,
        system_prompt,
        seed_content,
        worker_id: crate::infra::constants::DEFAULT_WORKER_ID.to_owned(),
        cache_engine_json: state.cache_engine_json.clone(),
    }
}

/// Default context initialization (panel creation, UID assignment, agent seeds).
mod init;
pub(crate) use init::{ensure_default_agent, ensure_default_contexts, get_active_agent_content};
