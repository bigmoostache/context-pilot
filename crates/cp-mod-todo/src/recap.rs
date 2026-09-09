//! Conversation-level collapsing of superseded task recaps.
//!
//! Every task recap emitted into a tool result — by the `Todo` tool itself, or
//! appended to a tool whose `task_id` flipped a task — is produced by
//! [`result_annex`](crate::tree::result_annex) and framed by
//! [`RECAP_OPEN`](crate::tree::RECAP_OPEN) /
//! [`RECAP_CLOSE`](crate::tree::RECAP_CLOSE). Each new recap supersedes every
//! earlier one: they are snapshots of the same tree, so all but the most recent
//! are stale.
//!
//! [`strip_superseded`] walks the **live conversation** (never the frozen
//! `ConversationHistory` panels, whose content is already compacted) and
//! replaces every recap but the last with a one-line stub. On a long session
//! this reclaims a few hundred tokens per superseded call and, more importantly,
//! leaves exactly ONE task tree in context — so the model can't mistake a stale
//! snapshot for current state.
//!
//! It lives beside the producer on purpose: writer and reader share the same
//! sentinel constants, so the framing can never drift.
//!
//! # The caller owns the gating
//!
//! Rewriting old messages mutates the conversation **prefix**, which invalidates
//! the LLM prompt cache from the earliest edited message onward. That is only
//! acceptable on a tick where the cache is already being broken — the host crate
//! calls this only when the queue is idle (no half-formed batch) and tempo is
//! already broken. Do not call it on a tempo-preserving tick.
//!
//! The pass only rewrites `content` (what the model reads), never `display`
//! (what the TUI renders), so the human keeps full scrollback.

use cp_base::state::runtime::State;

use crate::tree::{RECAP_CLOSE, RECAP_OPEN};

/// Replaces a superseded recap. Carries no sentinel, so a stubbed result is
/// invisible to the next scan — the pass is naturally idempotent.
const STUB: &str = "[task recap superseded]";

/// Collapse every task recap in the live conversation except the most recent.
///
/// Returns the number of recaps stubbed (0 when there was nothing to do — i.e.
/// zero or one recap in the whole conversation).
pub fn strip_superseded(state: &mut State) -> usize {
    let total = count_recaps(state);
    if total <= 1 {
        return 0; // Nothing superseded — the only recap is the live one.
    }

    // Collapse the first `total - 1` occurrences in conversation order; the
    // final one is the live tree and stays untouched.
    let mut budget = total.saturating_sub(1);
    let mut stripped = 0usize;
    for msg in &mut state.messages {
        for record in &mut msg.tool_results {
            if budget == 0 {
                return stripped;
            }
            let (rewritten, n) = collapse(&record.content, budget);
            if n > 0 {
                record.content = rewritten;
                budget = budget.saturating_sub(n);
                stripped = stripped.saturating_add(n);
            }
        }
    }
    stripped
}

/// Total number of well-formed recap blocks across the live conversation.
fn count_recaps(state: &State) -> usize {
    state
        .messages
        .iter()
        .flat_map(|m| m.tool_results.iter())
        .map(|r| collapse(&r.content, usize::MAX).1)
        .fold(0usize, usize::saturating_add)
}

/// Replace up to `budget` recap blocks in `content` with [`STUB`], scanning left
/// to right. Returns the rewritten string and how many were replaced.
///
/// An unterminated `RECAP_OPEN` (truncated content) stops the scan and leaves
/// the remainder verbatim — never deletes to end-of-string.
fn collapse(content: &str, budget: usize) -> (String, usize) {
    if budget == 0 || !content.contains(RECAP_OPEN) {
        return (content.to_owned(), 0);
    }
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    let mut count = 0usize;
    while count < budget {
        let Some(start) = rest.find(RECAP_OPEN) else { break };
        let after_open = start.saturating_add(RECAP_OPEN.len());
        let Some(offset) = rest.get(after_open..).and_then(|tail| tail.find(RECAP_CLOSE)) else {
            break; // Unterminated block — leave the remainder untouched.
        };
        let end = after_open.saturating_add(offset).saturating_add(RECAP_CLOSE.len());
        out.push_str(rest.get(..start).unwrap_or_default());
        out.push_str(STUB);
        rest = rest.get(end..).unwrap_or_default();
        count = count.saturating_add(1);
    }
    out.push_str(rest);
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::{RECAP_CLOSE, RECAP_OPEN, STUB, collapse, strip_superseded};

    /// Build a recap block with `body` inside the sentinels.
    fn block(body: &str) -> String {
        format!("{RECAP_OPEN}\n{body}\n{RECAP_CLOSE}")
    }

    #[test]
    fn collapse_replaces_up_to_budget_in_order() {
        let content = format!("head {} mid {} tail", block("one"), block("two"));
        let (out, n) = collapse(&content, 1);
        assert_eq!(n, 1);
        assert_eq!(out, format!("head {STUB} mid {} tail", block("two")));
    }

    #[test]
    fn collapse_is_idempotent_on_stubbed_content() {
        let (once, _n) = collapse(&block("one"), 1);
        let (twice, n) = collapse(&once, 1);
        assert_eq!(n, 0);
        assert_eq!(once, twice);
    }

    #[test]
    fn collapse_leaves_unterminated_block_intact() {
        let content = format!("head {RECAP_OPEN} dangling");
        let (out, n) = collapse(&content, 1);
        assert_eq!(n, 0);
        assert_eq!(out, content);
    }

    #[test]
    fn collapse_preserves_surrounding_text() {
        let content = format!("Todo applied.\n\n{}", block("tree"));
        let (out, n) = collapse(&content, 1);
        assert_eq!(n, 1);
        assert_eq!(out, format!("Todo applied.\n\n{STUB}"));
    }

    #[test]
    fn collapse_zero_budget_is_a_noop() {
        let content = block("one");
        let (out, n) = collapse(&content, 0);
        assert_eq!(n, 0);
        assert_eq!(out, content);
    }

    // ── End-to-end over a real `State` ───────────────────────────────────
    //
    // The `collapse` tests above pin the string surgery; these pin the part
    // that actually ships: walking `state.messages`, keeping the LAST recap
    // across messages, and leaving `display` untouched.

    use cp_base::state::data::message::{Message, ToolResultRecord};
    use cp_base::state::runtime::State;

    /// A tool-result message carrying one record whose content is `content`.
    fn result_msg(id: &str, content: String) -> Message {
        let record = ToolResultRecord::new(format!("{id}_use"), content, false);
        Message::new_tool_result(id.to_owned(), None, vec![record])
    }

    /// The content of every tool-result record, in conversation order.
    fn contents(state: &State) -> Vec<String> {
        state.messages.iter().flat_map(|m| m.tool_results.iter()).map(|r| r.content.clone()).collect()
    }

    #[test]
    fn strips_every_recap_but_the_last_across_messages() {
        let mut state = State::default();
        state.messages.push(result_msg("R1", format!("Todo applied.\n\n{}", block("first"))));
        state.messages.push(result_msg("R2", format!("Todo applied.\n\n{}", block("second"))));
        state.messages.push(result_msg("R3", format!("Todo applied.\n\n{}", block("third"))));

        assert_eq!(strip_superseded(&mut state), 2);

        let got = contents(&state);
        let mut seen = got.iter();
        // The two older recaps collapse; their "Todo applied." survives.
        assert_eq!(seen.next(), Some(&format!("Todo applied.\n\n{STUB}")));
        assert_eq!(seen.next(), Some(&format!("Todo applied.\n\n{STUB}")));
        // The most recent one is the live tree — untouched.
        assert_eq!(seen.next(), Some(&format!("Todo applied.\n\n{}", block("third"))));
    }

    #[test]
    fn single_recap_is_left_alone() {
        let mut state = State::default();
        let only = format!("Todo applied.\n\n{}", block("only"));
        state.messages.push(result_msg("R1", only.clone()));

        assert_eq!(strip_superseded(&mut state), 0);
        assert_eq!(contents(&state).first(), Some(&only));
    }

    #[test]
    fn repeated_passes_are_idempotent() {
        let mut state = State::default();
        state.messages.push(result_msg("R1", block("first")));
        state.messages.push(result_msg("R2", block("second")));

        assert_eq!(strip_superseded(&mut state), 1);
        let after_first = contents(&state);
        // Second pass finds a single surviving recap → nothing left to do.
        assert_eq!(strip_superseded(&mut state), 0);
        assert_eq!(contents(&state), after_first);
    }

    #[test]
    fn display_is_never_rewritten() {
        let mut state = State::default();
        let full = block("first");
        let record = ToolResultRecord::new("u1".to_owned(), full.clone(), false).display(Some(full.clone()));
        state.messages.push(Message::new_tool_result("R1".to_owned(), None, vec![record]));
        state.messages.push(result_msg("R2", block("second")));

        assert_eq!(strip_superseded(&mut state), 1);

        let first = state.messages.first().and_then(|m| m.tool_results.first()).expect("first result record");
        assert_eq!(first.content, STUB, "content collapses (the model's view)");
        assert_eq!(first.display.as_deref(), Some(full.as_str()), "display keeps the human's scrollback");
    }
}
