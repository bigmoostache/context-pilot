use cp_base::cast::Safe as _;
use cp_base::state::data::model_helpers::ModelPricing as _;

use crate::app::panels::{ContextItem, collect_all_context, refresh_all_panels};
use crate::infra::tools::ToolDefinition;
use crate::infra::tools::refresh_conversation_context;
use crate::modules;
use crate::state::cache::hash_content;
use crate::state::{Message, State};
use cp_base::state::data::{CacheBreakKind, TickTelemetry};

mod detach;
/// Freeze policy: per-panel and ordering freeze decisions (queue, tempo, breath budget).
mod freeze;
use freeze::{FreezeDecision, freeze_conditions};

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

    if let (true, Some(snapshot)) = (full_freeze, &state.frozen_context_snapshot) {
        // ═══ FULL FREEZE: replay exact previous prompt ═══════════════════════
        //
        // Replace all panel items with the stored snapshot. The "chat" item
        // (conversation body) is preserved from the fresh context — new messages
        // are naturally appended at the tail where they don't break the panel
        // cache prefix.
        let chat_item = context_items.iter().find(|i| i.id == "chat").cloned();
        context_items.clone_from(snapshot);
        if let Some(chat) = chat_item {
            context_items.push(chat);
        }

        // Simplified telemetry — everything is a cache hit, no break
        let three_last_tools: String = state
            .messages
            .iter()
            .rev()
            .filter(|m| m.msg_type == crate::state::MsgKind::ToolCall)
            .flat_map(|m| m.tool_uses.iter().map(|t| t.name.as_str()))
            .filter(|name| *name != "Tool_execution")
            .take(3)
            .collect::<Vec<_>>()
            .join(",");

        let total_panel_tokens: usize = context_items
            .iter()
            .filter(|i| i.id != "chat")
            .map(|i| crate::state::estimate_tokens(&i.content))
            .sum();

        state.tick_telemetry = Some(TickTelemetry {
            tick_start_ms: crate::app::panels::now_ms(),
            three_last_tools,
            culprit_type: "none".to_string(),
            tokens_before_culprit: prompt_prefix_tokens.saturating_add(total_panel_tokens),
            tokens_culprit: 0,
            tokens_after_culprit: 0,
            queue_is_active: cond.queue_active,
            tempo_is_active: cond.tempo,
            break_kind: CacheBreakKind::NoBreak,
            culprit_max_freezes: 0,
        });
        // previous_panel_hash_list and previous_panel_id_types stay unchanged
    } else {
        // ═══ Normal path: per-panel freeze decisions ═════════════════════════

    {
        let mut cache_broken = false;
        let mut new_hash_list: Vec<String> = Vec::new();

        // Culprit tracking for tick telemetry
        let mut culprit_type: Option<String> = None;
        let mut culprit_panel_idx: Option<usize> = None;
        let mut culprit_max_freezes: u8 = 0;
        let mut culprit_is_new = false;
        let mut panel_token_counts: Vec<usize> = Vec::new();
        let mut panel_idx: usize = 0;

        let hit_price = state.cache_hit_price_per_mtok();
        let miss_price = state.cache_miss_price_per_mtok();

        for item in &mut context_items {
            // The conversation panel flows through messages, not the freeze system
            if item.id == "chat" {
                continue;
            }

            let fresh_hash = hash_content(&item.content);

            // Look up this panel's Entry
            let entry = state.context.iter_mut().find(|c| c.id == item.id);
            let Some(entry) = entry else {
                // Orphaned item (no Entry in state) — emit as-is, breaks cache
                new_hash_list.push(format!("{}:{fresh_hash}", item.id));
                if !cache_broken {
                    culprit_panel_idx = Some(panel_idx);
                    culprit_type = Some(item.id.clone());
                    culprit_max_freezes = 0; // orphaned panel — no Entry, no max_freezes
                    culprit_is_new = true; // orphaned = never emitted
                }
                cache_broken = true;
                panel_token_counts.push(crate::state::estimate_tokens(&item.content));
                panel_idx = panel_idx.saturating_add(1);
                continue;
            };

            // Detect change: compare fresh content hash to what was last emitted
            let last_hash = entry.emitted.hash.as_deref();
            let content_changed = last_hash.is_none_or(|lh| lh != fresh_hash);

            // The hash we'll record (may differ from fresh_hash if we freeze)
            let emitted_hash;

            if content_changed {
                // Content differs from last emission — consult freeze policy
                let panel = crate::app::panels::get_panel(&entry.context_type);
                let decision = cond.freeze_panel(cache_broken, entry.freeze_count, panel.max_freezes());

                if decision == FreezeDecision::Freeze
                    && let Some(ref frozen) = entry.emitted.context
                {
                    // FREEZE: restore the full snapshot (content + header + timestamp)
                    *item = frozen.clone();
                    entry.freeze_count = entry.freeze_count.saturating_add(1);
                    entry.total_freezes = entry.total_freezes.saturating_add(1);
                    emitted_hash = entry.emitted.hash.clone().unwrap_or(fresh_hash);
                } else {
                    // FRESH: emit new content (no snapshot, or policy says Fresh)
                    if !cache_broken {
                        culprit_panel_idx = Some(panel_idx);
                        culprit_type = Some(entry.context_type.to_string());
                        culprit_max_freezes = panel.max_freezes();
                        culprit_is_new = last_hash.is_none();
                    }
                    entry.freeze_count = 0;
                    entry.emitted.hash = Some(fresh_hash.clone());
                    entry.total_cache_misses = entry.total_cache_misses.saturating_add(1);
                    emitted_hash = fresh_hash;
                    cache_broken = true;
                }
            } else {
                // Content unchanged — cache preserved naturally, no action needed
                emitted_hash = fresh_hash;
            }

            // Snapshot what was ACTUALLY emitted (post-decision)
            entry.emitted.context = Some(item.clone());

            // Cost tracking: build hash list for prefix-match
            new_hash_list.push(format!("{}:{emitted_hash}", item.id));

            // Tick telemetry: track per-panel token counts for culprit decomposition
            panel_token_counts.push(crate::state::estimate_tokens(&item.content));
            panel_idx = panel_idx.saturating_add(1);
        }

        // Prefix-match for per-panel cache hit/miss cost tracking
        let prev = &state.previous_panel_hash_list;
        let prefix_len = new_hash_list.iter().zip(prev.iter()).take_while(|(a, b)| a == b).count();

        for (i, entry_str) in new_hash_list.iter().enumerate() {
            let panel_id = entry_str.split(':').next().unwrap_or("");
            let is_hit = i < prefix_len;
            let price = if is_hit { hit_price } else { miss_price };

            if let Some(ctx) = state.context.iter_mut().find(|c| c.id == panel_id) {
                let cost = ctx.token_count.to_f64() * f64::from(price) / 1_000_000.0;
                ctx.panel_cache_hit = is_hit;
                ctx.panel_total_cost += cost;
            }
        }

        state.previous_panel_hash_list = new_hash_list;

        // === Detect disappeared panels ===
        // A panel from SA that's missing from SB breaks the prompt even though
        // the freeze loop (which only iterates SB's panels) wouldn't detect it.
        let break_kind = if cache_broken {
            if culprit_is_new { CacheBreakKind::PanelAppeared } else { CacheBreakKind::ContentChanged }
        } else {
            let current_ids: std::collections::HashSet<&str> =
                context_items.iter().filter(|item| item.id != "chat").map(|item| item.id.as_str()).collect();
            let disappeared = state.previous_panel_id_types.iter().find(|(id, _)| !current_ids.contains(id.as_str()));
            if let Some((_gone_id, gone_type)) = disappeared {
                culprit_type = Some(gone_type.clone());
                // No panel_idx — disappeared panel isn't in current items.
                // Token decomposition: all current tokens go to "before".
                CacheBreakKind::PanelDisappeared
            } else {
                CacheBreakKind::NoBreak
            }
        };

        // Save panel (id, context_type) for next tick's disappearance detection
        state.previous_panel_id_types = context_items
            .iter()
            .filter(|item| item.id != "chat")
            .map(|item| {
                let ctx_type = state
                    .context
                    .iter()
                    .find(|c| c.id == item.id)
                    .map_or_else(|| item.id.clone(), |c| c.context_type.to_string());
                (item.id.clone(), ctx_type)
            })
            .collect();

        // === Tick telemetry: culprit decomposition + freeze flags ===
        let (tokens_before, tok_culprit, tokens_after) = culprit_panel_idx.map_or_else(
            || {
                let total: usize = panel_token_counts.iter().sum();
                (total, 0, 0)
            },
            |ci| {
                let before: usize = panel_token_counts.iter().take(ci).sum();
                let culprit = panel_token_counts.get(ci).copied().unwrap_or(0);
                let after: usize = panel_token_counts.iter().skip(ci.saturating_add(1)).sum();
                (before, culprit, after)
            },
        );

        // Last 3 tools: scan messages backward for ToolCall entries, skip synthetic Tool_execution stubs
        let three_last_tools: String = state
            .messages
            .iter()
            .rev()
            .filter(|m| m.msg_type == crate::state::MsgKind::ToolCall)
            .flat_map(|m| m.tool_uses.iter().map(|t| t.name.as_str()))
            .filter(|name| *name != "Tool_execution")
            .take(3)
            .collect::<Vec<_>>()
            .join(",");

        state.tick_telemetry = Some(TickTelemetry {
            tick_start_ms: crate::app::panels::now_ms(),
            three_last_tools,
            culprit_type: culprit_type.unwrap_or_else(|| "none".to_string()),
            tokens_before_culprit: prompt_prefix_tokens.saturating_add(tokens_before),
            tokens_culprit: tok_culprit,
            tokens_after_culprit: tokens_after,
            queue_is_active: cond.queue_active,
            tempo_is_active: cond.tempo,
            break_kind,
            culprit_max_freezes,
        });
    }

        // Save snapshot for next frozen tick (panels only, no "chat")
        state.frozen_context_snapshot = Some(
            context_items.iter().filter(|i| i.id != "chat").cloned().collect(),
        );
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
        // ── Reverie path ──
        // Add P-main-conv: the main worker's conversation as a read-only panel
        let main_conv_content = cp_base::state::data::message::format_messages_to_chunk(
            &state
                .messages
                .iter()
                .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
                .cloned()
                .collect::<Vec<_>>(),
        );
        context_items.push(ContextItem {
            id: "P-main-conv".to_string(),
            header: "Main Agent Conversation (read-only)".to_string(),
            content: main_conv_content,
            last_refresh_ms: crate::app::panels::now_ms(),
        });

        // Add P-reverie: agent prompt + context + tool restrictions + reverie conversation
        // Agent content is injected here (NOT as system prompt) to preserve cache hits.
        let mut reverie_panel_content = String::new();

        // Inject the reverie agent's prompt content
        {
            let agents = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Agent);
            if let Some(agent) = agents.iter().find(|a| a.id == rev.agent_id) {
                reverie_panel_content.push_str("## Agent Instructions\n");
                reverie_panel_content.push_str(&agent.content);
                reverie_panel_content.push('\n');
            }
            // Inject additional context from the reverie state
            if let Some(rev_state) = state.reveries.get(&rev.agent_id)
                && let Some(ctx) = &rev_state.context
            {
                reverie_panel_content.push_str("\n## Additional Context\n");
                reverie_panel_content.push_str(ctx);
                reverie_panel_content.push('\n');
            }
        }

        reverie_panel_content.push_str(&rev.tool_restrictions);
        if !rev.messages.is_empty() {
            reverie_panel_content
                .push_str("\n## Reverie Conversation\n(Your messages follow in the conversation below)\n");
        }
        context_items.push(ContextItem {
            id: "P-reverie".to_string(),
            header: "Reverie Context (tool restrictions + conversation)".to_string(),
            content: reverie_panel_content,
            last_refresh_ms: crate::app::panels::now_ms(),
        });

        // The reverie's messages ARE the conversation (may be empty on first run).
        // Tools are IDENTICAL to the main worker for prompt cache hits.
        // Report is described in the P-reverie panel text, not in the API tool list.
        let tools = state.tools.clone();
        // api_messages assembled later in start_streaming() from context_items + messages
        StreamContext { messages: rev.messages, context_items, tools }
    } else {
        // ── Main worker path ──
        let messages: Vec<_> = if include_last_message {
            state
                .messages
                .iter()
                .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
                .cloned()
                .collect()
        } else {
            state
                .messages
                .iter()
                .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
                .take(state.messages.len().saturating_sub(1))
                .cloned()
                .collect()
        };

        StreamContext { messages, context_items, tools: state.tools.clone() }
    }
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
        worker_id: crate::infra::constants::DEFAULT_WORKER_ID.to_string(),
        cache_engine_json: state.cache_engine_json.clone(),
    }
}

/// Default context initialization (panel creation, UID assignment, agent seeds).
mod init;
pub(crate) use init::{ensure_default_agent, ensure_default_contexts, get_active_agent_content};
