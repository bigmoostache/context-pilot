use ratatui::prelude::{Line, Span, Style};

use cp_base::config::accessors::theme;
use cp_base::panels::{ContextItem, Panel};
use cp_base::state::context::{ContextType, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::QueueState;
use std::fmt::Write as _;

/// Panel that renders the current queue status and queued tool calls.
pub(crate) struct QueuePanel;

impl Panel for QueuePanel {
    fn title(&self, state: &State) -> String {
        let qs = QueueState::get(state);
        if qs.active { format!("Queue ({})", qs.queued_calls.len()) } else { "Queue".to_string() }
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_context_text(state);
        let token_count = estimate_tokens(&content);
        for ctx in &mut state.context {
            if ctx.context_type.as_str() == ContextType::QUEUE {
                ctx.token_count = token_count;
                // Hash content and only bump last_refresh_ms when it actually changes.
                // This ensures the panel sorts correctly in context ordering —
                // unchanged panels stay near the top (cache-friendly), changed panels
                // float to the end (near conversation) so they don't break the prefix.
                let _ = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_context_text(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == ContextType::QUEUE)
            .map_or(("P11", 0), |c| (c.id.as_str(), c.last_refresh_ms));
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
        _ctx: &cp_base::state::context::ContextElement,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }
    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut cp_base::state::context::ContextElement,
        _state: &mut State,
    ) -> bool {
        false
    }
    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }
    fn suicide(&self, _ctx: &cp_base::state::context::ContextElement, _state: &State) -> bool {
        false
    }
    fn render(&self, _frame: &mut ratatui::Frame<'_>, _state: &mut State, _area: ratatui::prelude::Rect) {}

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let qs = QueueState::get(state);
        let accent = theme::accent();
        let muted = theme::text_muted();
        let warning = theme::warning();

        let mut lines = Vec::new();

        // Always show the shared-queue note
        lines.push(Line::from(Span::styled(
            "  This queue is shared between the main consciousness and background",
            Style::default().fg(muted).italic(),
        )));
        lines.push(Line::from(Span::styled(
            "  reverie sub-agents. Items may appear here from either source.",
            Style::default().fg(muted).italic(),
        )));
        lines.push(Line::from(""));

        if !qs.active && qs.queued_calls.is_empty() {
            lines.push(Line::from(Span::styled("  Queue inactive.", Style::default().fg(muted))));
            return lines;
        }

        // Status header
        let status = if qs.active { "Active" } else { "Paused" };
        let status_color = if qs.active { accent } else { warning };
        lines.push(Line::from(vec![
            Span::styled("  Queue ", Style::default().fg(muted)),
            Span::styled(status, Style::default().fg(status_color).bold()),
            Span::styled(format!(" — {} action(s)", qs.queued_calls.len()), Style::default().fg(muted)),
        ]));
        lines.push(Line::from(""));

        // Queued calls list
        for call in &qs.queued_calls {
            let params = serde_json::to_string(&call.input).unwrap_or_default();
            let short = if params.len() > 80 {
                let mut end = 77;
                while !params.is_char_boundary(end) {
                    end = end.saturating_sub(1);
                }
                format!("{}...", params.get(..end).unwrap_or(""))
            } else {
                params
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {}. ", call.index), Style::default().fg(muted)),
                Span::styled(call.tool_name.clone(), Style::default().fg(accent).bold()),
                Span::styled(format!("({short})"), Style::default().fg(muted)),
            ]));
        }

        lines
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
            text.push_str("Queue active — 0 actions queued.\n");
        } else {
            let status = if qs.active { "Active" } else { "Paused" };
            let _r1 = write!(text, "Queue {} — {} action(s) queued:\n\n", status, qs.queued_calls.len());
            for call in &qs.queued_calls {
                let params = serde_json::to_string(&call.input).unwrap_or_default();
                let short = if params.len() > 120 {
                    let mut end = 117;
                    while !params.is_char_boundary(end) {
                        end = end.saturating_sub(1);
                    }
                    format!("{}...", params.get(..end).unwrap_or(""))
                } else {
                    params
                };
                let _r2 = writeln!(text, "{}. {}({})", call.index, call.tool_name, short);
            }
        }
        text
    }
}
