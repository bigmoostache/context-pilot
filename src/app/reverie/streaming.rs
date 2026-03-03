//! Reverie streaming — prompt construction and LLM stream management.
//!
//! Uses the EXACT SAME prepare_stream_context() as the main worker, passing
//! a ReverieContext to branch only at the conversation section. This preserves
//! prompt prefix cache hits (panels + tools identical).

use std::sync::mpsc::Sender;

use crate::app::context::{ReverieContext, prepare_stream_context};
use crate::infra::api::{StreamParams, start_streaming};
use crate::infra::constants::DEFAULT_WORKER_ID;
use crate::state::State;
use cp_base::config::REVERIE;
use cp_base::llm_types::StreamEvent;

use super::tools;

/// Resolve the secondary model string from provider + model enum.
fn secondary_model_string(state: &State) -> String {
    use cp_base::llm_types::{LlmProvider, ModelInfo};
    match state.secondary_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
            state.secondary_anthropic_model.api_name().to_string()
        }
        LlmProvider::Grok => state.secondary_grok_model.api_name().to_string(),
        LlmProvider::Groq => state.secondary_groq_model.api_name().to_string(),
        LlmProvider::DeepSeek => state.secondary_deepseek_model.api_name().to_string(),
    }
}

/// Build the reverie prompt and start streaming to the secondary LLM.
///
/// Uses the exact same `prepare_stream_context()` as the main worker. The
/// `ReverieContext` parameter causes it to branch at the conversation section:
/// - Panels and tools are IDENTICAL → prompt prefix cache hit
/// - Conversation is replaced with P-main-conv + reverie's own messages
///
/// # Panics
/// Only call when `state.reveries` contains the given `agent_id`.
pub fn start_reverie_stream(state: &mut State, agent_id: &str, tx: Sender<StreamEvent>) {
    // Get the reverie's own messages (empty on first launch) and trim whitespace.
    // On first launch, inject a user kickoff message so the conversation starts
    // with a user turn — some models don't support assistant prefill.
    let mut reverie_messages = state.reveries.get(agent_id).map(|r| r.messages.clone()).unwrap_or_default();
    if reverie_messages.is_empty() {
        reverie_messages.push(cp_base::state::Message::new_user(
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

    // Use the EXACT same prepare_stream_context as the main worker.
    // Passing ReverieContext replaces the conversation section with
    // P-main-conv + reverie messages — panels and tools stay IDENTICAL for cache hits.
    let ctx = prepare_stream_context(
        state,
        true,
        Some(ReverieContext { agent_id: agent_id.to_string(), messages: reverie_messages, tool_restrictions }),
    );

    // Fire the stream to the secondary model
    start_streaming(
        StreamParams {
            provider: state.secondary_provider,
            model: secondary_model_string(state),
            max_output_tokens: state.secondary_max_output_tokens(),
            messages: ctx.messages,
            context_items: ctx.context_items,
            tools: ctx.tools,
            system_prompt: REVERIE.system_prompt.trim_end().to_string(),
            seed_content: None,
            worker_id: DEFAULT_WORKER_ID.to_string(),
        },
        tx,
    );
}
