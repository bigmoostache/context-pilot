use crate::state::{ContextType, MessageStatus, State, estimate_tokens};

/// Refresh token count for the Conversation context element
pub fn refresh_conversation_context(state: &mut State) {
    // Calculate total tokens from all active messages
    let total_tokens: usize = state
        .messages
        .iter()
        .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
        .map(|m| match m.status {
            MessageStatus::Summarized => m.tl_dr_token_count.max(estimate_tokens(m.tl_dr.as_deref().unwrap_or(""))),
            _ => m.content_token_count.max(estimate_tokens(&m.content)),
        })
        .sum();

    // Update the Conversation context element's token count
    for ctx in &mut state.context {
        if ctx.context_type == ContextType::CONVERSATION {
            ctx.token_count = total_tokens;
            break;
        }
    }
}
