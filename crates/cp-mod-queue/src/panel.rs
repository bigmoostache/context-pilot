use cp_base::panels::{ContextItem, Panel};
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::QueueState;
use std::fmt::Write as _;

/// Panel that renders the current queue status and queued tool calls.
pub(crate) struct QueuePanel;

impl Panel for QueuePanel {
    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let qs = QueueState::get(state);
        let mut blocks = Vec::new();

        blocks.push(Block::Line(vec![
            S::muted("  This queue is shared between the main consciousness and background".into()).italic(),
        ]));
        blocks.push(Block::Line(vec![
            S::muted("  reverie sub-agents. Reverie items are tagged; each side flushes only its own.".into()).italic(),
        ]));
        blocks.push(Block::empty());

        if !qs.active && qs.queued_calls.is_empty() {
            blocks.push(Block::Line(vec![S::muted("  Queue inactive.".into())]));
            return blocks;
        }

        let (status_text, status_sem) =
            if qs.active { ("Active", Semantic::Accent) } else { ("Paused", Semantic::Warning) };
        blocks.push(Block::Line(vec![
            S::muted("  Queue ".into()),
            S::styled(status_text.into(), status_sem).bold(),
            S::muted(format!(" — {} action(s)", qs.queued_calls.len())),
        ]));
        blocks.push(Block::empty());

        for call in &qs.queued_calls {
            blocks.push(Block::Line(vec![
                S::muted(format!("  {}. ", call.index)),
                S::accent(call.tool_name.clone()).bold(),
                S::muted(format!("({}){}", truncate_params(&call.input, 80), owner_tag(call))),
            ]));
        }

        blocks
    }
    fn title(&self, state: &State) -> String {
        let qs = QueueState::get(state);
        if qs.active { format!("Queue ({})", qs.queued_calls.len()) } else { "Queue".to_owned() }
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_context_text(state);
        let token_count = estimate_tokens(&content);
        for ctx in &mut state.agent_mut().context {
            if ctx.context_type.as_str() == Kind::QUEUE {
                ctx.token_count = token_count;
                // Hash content and only bump last_refresh_ms when it actually changes.
                // This ensures the panel sorts correctly in context ordering —
                // unchanged panels stay near the top (cache-friendly), changed panels
                // float to the end (near conversation) so they don't break the prefix.
                let _changed = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_context_text(state);
        let (id, last_refresh_ms) = state
            .agent()
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::QUEUE)
            .map_or(("", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Queue", content, last_refresh_ms)]
    }

    fn handle_key(&self, _key: &crossterm::event::KeyEvent, _state: &State) -> Option<cp_base::state::actions::Action> {
        None
    }
    fn needs_cache(&self) -> bool {
        false
    }
    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }
    fn build_cache_request(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }
    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut cp_base::state::context::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }
    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }
    fn suicide(&self, _ctx: &cp_base::state::context::Entry, _state: &State) -> bool {
        false
    }
}

impl QueuePanel {
    /// Shared text builder for both `refresh()` and `context()`
    fn format_context_text(state: &State) -> String {
        let qs = QueueState::get(state);
        let mut text = String::new();
        if !qs.active && qs.queued_calls.is_empty() {
            text.push_str("Queue inactive.\n");
        } else if qs.active && qs.queued_calls.is_empty() {
            text.push_str("Queue active \u{2014} 0 actions queued.\n");
        } else {
            let status = if qs.active { "Active" } else { "Paused" };
            let _r1 = write!(text, "Queue {} — {} action(s) queued:\n\n", status, qs.queued_calls.len());
            for call in &qs.queued_calls {
                let _r2 = writeln!(
                    text,
                    "{}. {}({}){}",
                    call.index,
                    call.tool_name,
                    truncate_params(&call.input, 120),
                    owner_tag(call)
                );
            }
        }
        text
    }
}

/// Compact JSON of a call's params, cut to `max` bytes (char-boundary safe, `...` suffix).
fn truncate_params(input: &serde_json::Value, max: usize) -> String {
    let params = serde_json::to_string(input).unwrap_or_default();
    if params.len() <= max {
        return params;
    }
    let mut end = max.saturating_sub(3);
    while !params.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", params.get(..end).unwrap_or(""))
}

/// ` [reverie:<id>]` for reverie-owned calls, empty for the main agent's.
fn owner_tag(call: &crate::types::QueuedToolCall) -> String {
    call.owner.as_ref().map_or_else(String::new, |o| format!(" [reverie:{o}]"))
}

#[cfg(test)]
mod tests {
    use super::truncate_params;

    #[test]
    fn truncate_keeps_short_params() {
        assert_eq!(truncate_params(&serde_json::json!({"a": 1i32}), 80), r#"{"a":1}"#);
    }

    #[test]
    fn truncate_cuts_on_char_boundary() {
        let out = truncate_params(&serde_json::json!({"k": "\u{e9}".repeat(100)}), 20);
        assert!(out.ends_with("..."));
        assert!(out.len() <= 20);
    }
}
