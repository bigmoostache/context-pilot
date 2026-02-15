use crate::persistence::log_error;
use crate::state::{ContextType, State, estimate_tokens};

use super::ActionResult;
use super::helpers::clean_llm_id_prefix;

/// Handle AppendChars action — append streaming text to assistant message
pub fn handle_append_chars(state: &mut State, text: &str) -> ActionResult {
    if let Some(msg) = state.messages.last_mut()
        && msg.role == "assistant"
    {
        msg.content.push_str(text);

        // Update estimated token count during streaming
        let new_estimate = estimate_tokens(&msg.content);
        let added = new_estimate.saturating_sub(state.streaming_estimated_tokens);

        if added > 0 {
            if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::CONVERSATION) {
                ctx.token_count += added;
            }
            state.streaming_estimated_tokens = new_estimate;
        }
    }
    ActionResult::Nothing
}

/// Handle StreamDone action — finalize streaming, correct token counts
pub fn handle_stream_done(
    state: &mut State,
    _input_tokens: usize,
    output_tokens: usize,
    cache_hit_tokens: usize,
    cache_miss_tokens: usize,
    stop_reason: &Option<String>,
) -> ActionResult {
    state.is_streaming = false;
    state.last_stop_reason = stop_reason.clone();

    // Set tick stats (this tick only)
    state.tick_cache_hit_tokens = cache_hit_tokens;
    state.tick_cache_miss_tokens = cache_miss_tokens;
    state.tick_output_tokens = output_tokens;

    // Accumulate per-stream stats (reset at InputSubmit)
    state.stream_cache_hit_tokens += cache_hit_tokens;
    state.stream_cache_miss_tokens += cache_miss_tokens;
    state.stream_output_tokens += output_tokens;

    // Accumulate total stats
    state.cache_hit_tokens += cache_hit_tokens;
    state.cache_miss_tokens += cache_miss_tokens;
    state.total_output_tokens += output_tokens;

    // Correct the estimated tokens with actual output tokens on Conversation context and update timestamp
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::CONVERSATION) {
        // Remove our estimate, add actual
        ctx.token_count =
            ctx.token_count.saturating_sub(state.streaming_estimated_tokens).saturating_add(output_tokens);
        ctx.last_refresh_ms = crate::core::panels::now_ms();
    }
    state.streaming_estimated_tokens = 0;

    // Store actual token count on message and clean up LLM prefixes
    if let Some(msg) = state.messages.last_mut()
        && msg.role == "assistant"
    {
        // Remove any [A##]: prefixes the LLM mistakenly added
        msg.content = clean_llm_id_prefix(&msg.content);
        msg.content_token_count = output_tokens;
        msg.input_tokens = _input_tokens;
        let id = msg.id.clone();
        return ActionResult::SaveMessage(id);
    }
    ActionResult::Save
}

/// Handle StreamError action — clean up streaming state, log error
pub fn handle_stream_error(state: &mut State, error: &str) -> ActionResult {
    state.is_streaming = false;

    // Remove estimated tokens on error from Conversation context
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::CONVERSATION) {
        ctx.token_count = ctx.token_count.saturating_sub(state.streaming_estimated_tokens);
    }
    state.streaming_estimated_tokens = 0;

    // Log error to file
    let error_file = log_error(error);

    if let Some(msg) = state.messages.last_mut()
        && msg.role == "assistant"
    {
        msg.content = format!("[Error occurred. See details in {}]", error_file);
        let id = msg.id.clone();
        return ActionResult::SaveMessage(id);
    }
    ActionResult::Save
}
