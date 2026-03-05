use crate::state::{Kind, MsgStatus, State, estimate_tokens};

/// Estimate total tokens for a single message, including content, tool uses, and tool results.
pub(crate) fn estimate_message_tokens(m: &crate::state::Message) -> usize {
    let content_tokens = m.content_token_count.max(estimate_tokens(&m.content));

    // Count tool uses (tool call name + JSON input)
    let tool_use_tokens: usize = m
        .tool_uses
        .iter()
        .map(|tu| {
            let input_str = serde_json::to_string(&tu.input).unwrap_or_default();
            estimate_tokens(&tu.name).saturating_add(estimate_tokens(&input_str))
        })
        .sum();

    // Count tool results
    let tool_result_tokens: usize = m.tool_results.iter().map(|tr| estimate_tokens(&tr.content)).sum();

    content_tokens.saturating_add(tool_use_tokens).saturating_add(tool_result_tokens)
}

/// Refresh token count for the Conversation context element
pub(crate) fn refresh_conversation_context(state: &mut State) {
    // Calculate total tokens from all active messages (content + tool uses + tool results)
    let total_tokens: usize = state
        .messages
        .iter()
        .filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
        .map(estimate_message_tokens)
        .sum();

    // Update the Conversation context element's token count
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == Kind::CONVERSATION {
            ctx.token_count = total_tokens;
            break;
        }
    }
}
