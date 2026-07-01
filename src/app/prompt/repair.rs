//! Tool-pairing repair — the authoritative enforcement of the Anthropic
//! tool-call adjacency invariant on an assembled prompt.
//!
//! The Anthropic API requires that every assistant message containing
//! `tool_use` blocks be answered by a `tool_result` for each id in the
//! *immediately* following user message. The main assembler
//! ([`super::builder`]) only guards *existence* (a matching result
//! somewhere later, a matching call somewhere earlier) — a reshuffled or
//! truncated history can satisfy those checks while still breaking adjacency,
//! producing a `messages.N: tool_use ids ... without tool_result blocks
//! immediately after` 400.
//!
//! [`repair_tool_pairing`] runs as the final assembly phase and self-heals such
//! histories so an orphaned `tool_use` can never reach the API.

use crate::llms::{ApiMessage, ContentBlock};

/// Placeholder content for a `tool_result` synthesized to pair an orphaned
/// `tool_use` (call recorded but its result was lost to an interrupt, reload,
/// truncation, or partial queue flush).
const MISSING_TOOL_RESULT: &str = "(no result recorded — tool call was interrupted or its result was lost)";

/// Collect the `tool_use` ids carried by a message, in order.
fn tool_use_ids_of(msg: &ApiMessage) -> Vec<String> {
    msg.content
        .iter()
        .filter_map(|b| if let ContentBlock::ToolUse { id, .. } = b { Some(id.clone()) } else { None })
        .collect()
}

/// Collect the `tool_result` ids carried by a message, in order.
fn tool_result_ids_of(msg: &ApiMessage) -> Vec<String> {
    msg.content
        .iter()
        .filter_map(
            |b| if let ContentBlock::ToolResult { tool_use_id, .. } = b { Some(tool_use_id.clone()) } else { None },
        )
        .collect()
}

/// Build a single synthetic placeholder `tool_result` for an orphaned `tool_use` id.
fn placeholder_result(id: &str) -> ContentBlock {
    ContentBlock::ToolResult { tool_use_id: id.to_string(), content: MISSING_TOOL_RESULT.to_string() }
}

/// Build synthetic placeholder `tool_result` blocks for a set of `tool_use` ids.
fn placeholder_results(ids: &[String]) -> Vec<ContentBlock> {
    ids.iter().map(|id| placeholder_result(id)).collect()
}

/// Enforce the Anthropic tool-call adjacency invariant on assembled messages.
///
/// For every assistant message containing `tool_use` blocks, guarantees the
/// message immediately after is a user message whose leading `tool_result`
/// blocks cover each `tool_use` id:
/// - missing results are prepended as synthetic placeholders (in `tool_use` order);
/// - a user message is inserted when none follows;
/// - stray `tool_result` blocks whose id has no matching `tool_use` in the
///   immediately-preceding assistant message are dropped (the symmetric invariant).
pub(super) fn repair_tool_pairing(messages: &mut Vec<ApiMessage>) {
    // Forward pass: every assistant tool_use gets an adjacent result.
    let mut idx = 0;
    while idx < messages.len() {
        let tool_use_ids = messages.get(idx).map(tool_use_ids_of).unwrap_or_default();
        if tool_use_ids.is_empty() {
            idx = idx.saturating_add(1);
            continue;
        }

        let next = idx.saturating_add(1);
        let Some(next_msg) = messages.get(next) else {
            // No message follows — append one carrying placeholders for all ids.
            messages.push(ApiMessage { role: "user".to_string(), content: placeholder_results(&tool_use_ids) });
            idx = idx.saturating_add(2);
            continue;
        };

        if next_msg.role != "user" {
            // A non-user message follows — insert a user result message between them.
            messages.insert(next, ApiMessage { role: "user".to_string(), content: placeholder_results(&tool_use_ids) });
            idx = idx.saturating_add(2);
            continue;
        }

        // A user message follows — prepend placeholders for any uncovered id.
        let present: std::collections::HashSet<String> = tool_result_ids_of(next_msg).into_iter().collect();
        let mut prepend: Vec<ContentBlock> =
            tool_use_ids.iter().filter(|id| !present.contains(*id)).map(|id| placeholder_result(id)).collect();
        if !prepend.is_empty()
            && let Some(next_mut) = messages.get_mut(next)
        {
            prepend.append(&mut next_mut.content);
            next_mut.content = prepend;
        }
        idx = idx.saturating_add(2);
    }

    // Symmetric pass: drop stray tool_results whose call isn't in the message before.
    for pos in 1..messages.len() {
        let prior_ids: std::collections::HashSet<String> =
            messages.get(pos.saturating_sub(1)).map(tool_use_ids_of).unwrap_or_default().into_iter().collect();
        let Some(msg) = messages.get_mut(pos) else { continue };
        if msg.role != "user" {
            continue;
        }
        let has_stray = msg
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { tool_use_id, .. } if !prior_ids.contains(tool_use_id)));
        if has_stray {
            msg.content.retain(|b| match b {
                ContentBlock::ToolResult { tool_use_id, .. } => prior_ids.contains(tool_use_id),
                ContentBlock::Text { .. } | ContentBlock::ToolUse { .. } => true,
            });
        }
    }
    // Prune any user message left empty by stray-result removal.
    messages.retain(|m| !(m.role == "user" && m.content.is_empty()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tu(id: &str) -> ContentBlock {
        ContentBlock::ToolUse { id: id.to_string(), name: "t".to_string(), input: serde_json::Value::Null }
    }
    fn tr(id: &str) -> ContentBlock {
        ContentBlock::ToolResult { tool_use_id: id.to_string(), content: "ok".to_string() }
    }
    fn text(t: &str) -> ContentBlock {
        ContentBlock::Text { text: t.to_string() }
    }
    fn asst(content: Vec<ContentBlock>) -> ApiMessage {
        ApiMessage { role: "assistant".to_string(), content }
    }
    fn user(content: Vec<ContentBlock>) -> ApiMessage {
        ApiMessage { role: "user".to_string(), content }
    }

    /// Ids of `tool_result` blocks in a message, in order.
    fn result_ids(msg: &ApiMessage) -> Vec<String> {
        tool_result_ids_of(msg)
    }

    /// True if the message carries a text block equal to `needle`.
    fn has_text(msg: &ApiMessage, needle: &str) -> bool {
        msg.content.iter().any(|b| matches!(b, ContentBlock::Text { text } if text == needle))
    }

    #[test]
    fn missing_result_gets_placeholder() {
        // assistant(tool_use A) followed by a user text with no result.
        let mut msgs = vec![asst(vec![tu("A")]), user(vec![text("hi")])];
        repair_tool_pairing(&mut msgs);
        // The following user message now leads with a synthetic result for A.
        assert_eq!(msgs.get(1).map(result_ids), Some(vec!["A".to_string()]));
        // Original text is preserved after the injected result.
        assert_eq!(msgs.get(1).map(|m| has_text(m, "hi")), Some(true));
    }

    #[test]
    fn interleaved_result_breaks_adjacency_and_is_repaired() {
        // The exact reported bug: flush_1's result lands two messages away because
        // an unrelated result message is interleaved before flush_2's call.
        let mut msgs = vec![
            asst(vec![tu("flush_1")]),
            user(vec![tr("X")]), // stray result for an unrelated (already-paired) call
            asst(vec![tu("flush_2")]),
            user(vec![tr("flush_1"), tr("flush_2")]),
        ];
        repair_tool_pairing(&mut msgs);
        // flush_1's assistant message (idx 0) must now be immediately followed by a
        // user message carrying flush_1's result.
        assert_eq!(msgs.first().map(|m| m.role.as_str()), Some("assistant"));
        assert_eq!(msgs.first().map(tool_use_ids_of), Some(vec!["flush_1".to_string()]));
        assert_eq!(msgs.get(1).map(|m| m.role.as_str()), Some("user"));
        assert_eq!(msgs.get(1).map(|m| result_ids(m).contains(&"flush_1".to_string())), Some(true));
    }

    #[test]
    fn no_following_message_inserts_user() {
        // assistant(tool_use A) is the last message — a user result must be appended.
        let mut msgs = vec![asst(vec![text("done"), tu("A")])];
        repair_tool_pairing(&mut msgs);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs.get(1).map(|m| m.role.as_str()), Some("user"));
        assert_eq!(msgs.get(1).map(result_ids), Some(vec!["A".to_string()]));
    }

    #[test]
    fn valid_pairing_is_untouched() {
        let mut msgs = vec![asst(vec![tu("A"), tu("B")]), user(vec![tr("A"), tr("B")])];
        let before = msgs.clone();
        repair_tool_pairing(&mut msgs);
        assert_eq!(msgs.len(), before.len());
        assert_eq!(msgs.get(1).map(result_ids), Some(vec!["A".to_string(), "B".to_string()]));
        // No placeholder content injected.
        let has_placeholder = msgs.get(1).map(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolResult { content, .. } if content == MISSING_TOOL_RESULT))
        });
        assert_eq!(has_placeholder, Some(false));
    }

    #[test]
    fn stray_result_without_matching_call_is_dropped() {
        // A user message carries a tool_result whose call isn't in the message before.
        let mut msgs = vec![asst(vec![text("hello")]), user(vec![tr("ghost"), text("keep")])];
        repair_tool_pairing(&mut msgs);
        // The ghost result is stripped; the text survives.
        assert_eq!(msgs.get(1).map(|m| result_ids(m).is_empty()), Some(true));
        assert_eq!(msgs.get(1).map(|m| has_text(m, "keep")), Some(true));
    }

    #[test]
    fn placeholder_uses_missing_result_content() {
        let mut msgs = vec![asst(vec![tu("A")]), user(vec![text("x")])];
        repair_tool_pairing(&mut msgs);
        let injected = msgs.get(1).and_then(|m| {
            m.content.iter().find_map(|b| {
                if let ContentBlock::ToolResult { content, .. } = b { Some(content.clone()) } else { None }
            })
        });
        assert_eq!(injected.as_deref(), Some(MISSING_TOOL_RESULT));
    }
}
