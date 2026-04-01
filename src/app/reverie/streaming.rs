//! Reverie streaming — prompt construction and LLM stream management.
//!
//! Uses the EXACT SAME `prepare_stream_context()` as the main worker, passing
//! a `ReverieContext` to branch only at the conversation section. This preserves
//! prompt prefix cache hits (panels + tools identical).

use std::sync::mpsc::Sender;

use crate::app::context::{ReverieContext, build_stream_params, prepare_stream_context};
use crate::infra::api::start_streaming;
use crate::state::State;
use cp_base::config::REVERIE;
use cp_base::config::llm_types::StreamEvent;

use super::tools;

/// Build the reverie prompt and start streaming to the secondary LLM.
///
/// Uses the exact same `prepare_stream_context()` as the main worker. The
/// `ReverieContext` parameter causes it to branch at the conversation section:
/// - Panels and tools are IDENTICAL → prompt prefix cache hit
/// - Conversation is replaced with P-main-conv + reverie's own messages
///
/// # Panics
/// Only call when `state.reveries` contains the given `agent_id`.
pub(crate) fn start_reverie_stream(state: &mut State, agent_id: &str, tx: Sender<StreamEvent>) {
    // Get the reverie's own messages (empty on first launch) and trim whitespace.
    // On first launch, inject a user kickoff message so the conversation starts
    // with a user turn — some models don't support assistant prefill.
    let mut reverie_messages = state.reveries.get(agent_id).map(|r| r.messages.clone()).unwrap_or_default();
    if reverie_messages.is_empty() {
        reverie_messages.push(cp_base::state::data::message::Message::new_user(
            "reverie-kickoff".to_string(),
            "reverie-kickoff".to_string(),
            REVERIE.kickoff_message.trim_end().to_string(),
            0,
        ));
    }
    for msg in &mut reverie_messages {
        if msg.role == "assistant" {
            msg.content = msg.content.trim_end().to_string();
        }
    }

    // Build tool restrictions text for the reverie's conversation preamble
    let tool_restrictions = tools::build_tool_restrictions_text(&state.tools);

    // Build the reverie seed BEFORE prepare_stream_context, since tool_restrictions
    // will be moved into ReverieContext. The seed is the reverie's identity injection
    // (agent instructions + directive + tool restrictions), diverging from the main
    // worker's seed while sharing the exact same system prompt + panels + tools.
    let reverie_seed = build_reverie_seed(state, agent_id, &tool_restrictions);

    // Use the EXACT same prepare_stream_context as the main worker.
    // Passing ReverieContext replaces the conversation section with
    // P-main-conv + reverie messages — panels and tools stay IDENTICAL for cache hits.
    let ctx = prepare_stream_context(
        state,
        true,
        Some(ReverieContext { agent_id: agent_id.to_string(), messages: reverie_messages, tool_restrictions }),
    );

    // Fire the stream using the SAME provider/model/system prompt as the main worker.
    // Cache sharing requires identical prefix: system_prompt + panels + tools.
    // The ONLY divergence is seed_content (reverie identity vs main agent identity).
    let params = build_stream_params(state, ctx, Some(reverie_seed));
    start_streaming(params, tx);
}

/// Build the reverie's seed content — its identity injection after the shared panel prefix.
///
/// Contains the reverie agent's instructions, any user-provided directive, and tool
/// restrictions. This is what makes the reverie behave differently from the main worker
/// despite sharing the exact same system prompt, panels, and tools.
fn build_reverie_seed(state: &State, agent_id: &str, tool_restrictions: &str) -> String {
    let mut seed = String::new();

    // Agent instructions (cleaner / cartographer / etc.)
    {
        let ps = cp_mod_prompt::types::PromptState::get(state);
        if let Some(agent) = ps.agents.iter().find(|a| a.id == agent_id) {
            seed.push_str("## Reverie Agent Instructions\n");
            seed.push_str(&agent.content);
            seed.push('\n');
        }
    }

    // Additional context (directive from optimize_context tool)
    if let Some(rev_state) = state.reveries.get(agent_id)
        && let Some(ctx) = &rev_state.context
    {
        seed.push_str("\n## Directive\n");
        seed.push_str(ctx);
        seed.push('\n');
    }

    // Tool restrictions
    seed.push('\n');
    seed.push_str(tool_restrictions);

    seed
}
