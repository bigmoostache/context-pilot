use cp_base::cast::SafeCast as _;

use crate::app::panels::{ContextItem, collect_all_context, refresh_all_panels};
use crate::infra::tools::ToolDefinition;
use crate::infra::tools::refresh_conversation_context;
use crate::modules;
use crate::state::cache::hash_content;
use crate::state::{ContextType, Message, State, estimate_tokens};

mod detach;

/// Context data prepared for streaming
pub(super) struct StreamContext {
    pub messages: Vec<Message>,
    pub context_items: Vec<ContextItem>,
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
    // Mark UserMessage notifications as processed on every context rebuild.
    // This prevents the spine from firing a redundant auto-continuation for
    // messages the LLM already saw (e.g., user sent a message during a tool
    // call pause — the message is in context, LLM responds, but without this
    // the notification would still be "unprocessed" when the stream ends).
    cp_mod_spine::SpineState::mark_user_message_notifications_processed(state);

    // Detach old conversation chunks before anything else
    detach::detach_conversation_chunks(state);

    // Refresh conversation token counts (not panel-based yet)
    refresh_conversation_context(state);

    // Refresh all panel token counts
    refresh_all_panels(state);

    // Collect all context items from panels
    let mut context_items = collect_all_context(state);

    // Sort panels by last_refresh_ms ascending (oldest first, newest closest
    // to conversation). This ordering determines prompt caching: the LLM
    // provider sees panels in this order, and Anthropic-style prefix caching
    // means earlier panels are more likely to be cache hits.
    context_items.sort_by_key(|item| item.last_refresh_ms);

    // === Panel cache cost tracking ===
    // Hash each panel's content (what the LLM literally sees), build an ordered
    // hash list, compare to previous tick's list via prefix matching, and
    // accumulate per-panel costs based on cache hit/miss pricing.
    {
        // Build hash list from panel content (excluding "chat" which is conversation)
        let panel_hashes: Vec<(String, String, usize)> = context_items
            .iter()
            .filter(|item| item.id != "chat")
            .map(|item| {
                let h = hash_content(&item.content);
                (item.id.clone(), h, estimate_tokens(&item.content))
            })
            .collect();

        let new_hash_list: Vec<String> = panel_hashes.iter().map(|(id, h, _)| format!("{id}:{h}")).collect();

        // Find max prefix match index
        let prev = &state.previous_panel_hash_list;
        let prefix_len = new_hash_list.iter().zip(prev.iter()).take_while(|(a, b)| a == b).count();

        // Get pricing from current model
        let hit_price = state.cache_hit_price_per_mtok();
        let miss_price = state.cache_miss_price_per_mtok();

        // Update each panel's cache hit status and accumulate cost
        for (i, (panel_id, _, token_count)) in panel_hashes.iter().enumerate() {
            let is_hit = i < prefix_len;
            let price = if is_hit { hit_price } else { miss_price };
            let cost = (*token_count).to_f64() * f64::from(price) / 1_000_000.0;

            if let Some(ctx) = state.context.iter_mut().find(|c| c.id == *panel_id) {
                ctx.panel_cache_hit = is_hit;
                ctx.panel_total_cost += cost;
            }
        }

        // Store hash list for next tick
        state.previous_panel_hash_list = new_hash_list;
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
            let ps = cp_mod_prompt::PromptState::get(state);
            if let Some(agent) = ps.agents.iter().find(|a| a.id == rev.agent_id) {
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

// ─── Initialization ─────────────────────────────────────────────────────────

// Re-export agent/seed functions from prompt module
pub(crate) use cp_mod_prompt::seed::{ensure_default_agent, get_active_agent_content};

/// Assign a UID to a panel if it doesn't have one
fn assign_panel_uid(state: &mut State, context_type: &str) {
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == context_type)
        && ctx.uid.is_none()
    {
        ctx.uid = Some(format!("UID_{}_P", state.global_next_uid));
        state.global_next_uid += 1;
    }
}

/// Ensure all default context elements exist with correct IDs.
/// Uses the module registry to determine which fixed panels to create.
/// Conversation is special: it's always created but not numbered (no Px ID in sidebar).
/// P1 = Todo, P2 = Library, P3 = Overview, P4 = Tree, P5 = Memory,
/// P6 = Spine, P7 = Logs, P8 = Git, P9 = Scratchpad
pub(crate) fn ensure_default_contexts(state: &mut State) {
    // Ensure Conversation exists (special: no numbered Px, always first in context list)
    if !state.context.iter().any(|c| c.context_type.as_str() == ContextType::CONVERSATION) {
        let elem =
            modules::make_default_context_element("chat", ContextType::new(ContextType::CONVERSATION), "Chat", true);
        state.context.insert(0, elem);
    }

    let defaults = modules::all_fixed_panel_defaults();

    for (pos, d) in defaults.iter().enumerate() {
        // Core modules always get their panels; non-core only if active
        if !d.is_core && !state.active_modules.contains(d.module_id) {
            continue;
        }

        // Skip if panel already exists
        if state.context.iter().any(|c| c.context_type == d.context_type) {
            continue;
        }

        // pos is 0-indexed in FIXED_PANEL_ORDER, but IDs start at P1
        let id = format!("P{}", pos + 1);
        let insert_pos = (pos + 1).min(state.context.len()); // +1 to account for Conversation at index 0
        let elem =
            modules::make_default_context_element(&id, d.context_type.clone(), d.display_name, d.cache_deprecated);
        state.context.insert(insert_pos, elem);
    }

    // Assign UID to Conversation (needed for panels/ storage — it holds message_uids)
    assign_panel_uid(state, ContextType::CONVERSATION);

    // Assign UIDs to all existing fixed panels (needed for panels/ storage)
    // Library panels don't need UIDs (rendered from in-memory state)
    for d in &defaults {
        if d.context_type.as_str() != ContextType::LIBRARY
            && state.context.iter().any(|c| c.context_type == d.context_type)
        {
            assign_panel_uid(state, d.context_type.as_str());
        }
    }
}
