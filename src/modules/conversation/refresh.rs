use crate::state::{Kind, MsgStatus, State, estimate_tokens};
use cp_base::config::constants::CHARS_PER_TOKEN;

/// Serialized JSON byte length of `value`, counted without building the string.
fn json_len(value: &serde_json::Value) -> usize {
    /// `io::Write` sink that only tallies bytes.
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(buf.len());
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    let _r = serde_json::to_writer(&mut counter, value);
    counter.0
}

/// Estimate total tokens for a single message, including content, tool uses, and tool results.
pub(crate) fn estimate_message_tokens(m: &crate::state::Message) -> usize {
    let content_tokens = m.content_token_count.max(estimate_tokens(&m.content));

    // Count tool uses (tool call name + JSON input)
    let tool_use_tokens: usize = m
        .tool_uses
        .iter()
        .map(|tu| {
            let input_tokens = cp_base::cast::float_math::ceil_ratio(json_len(&tu.input), CHARS_PER_TOKEN);
            estimate_tokens(&tu.name).saturating_add(input_tokens)
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
        .agent()
        .messages
        .iter()
        .filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
        .map(estimate_message_tokens)
        .sum();

    // Update the Conversation context element's token count
    for ctx in &mut state.agent_mut().context {
        if ctx.context_type.as_str() == Kind::CONVERSATION {
            ctx.token_count = total_tokens;
            break;
        }
    }
}
