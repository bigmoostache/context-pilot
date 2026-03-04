use ratatui::prelude::*;

use cp_base::config::theme;
use cp_base::panels::{ContextItem, Panel};
use cp_base::state::{ContextType, State, estimate_tokens};

use crate::types::QueueState;

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
            if ctx.context_type == ContextType::QUEUE {
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
            .find(|c| c.context_type == ContextType::QUEUE)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P11", 0));
        vec![ContextItem::new(id, "Queue", content, last_refresh_ms)]
    }

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
                    end -= 1;
                }
                format!("{}...", &params[..end])
            } else {
                params
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {}. ", call.index), Style::default().fg(muted)),
                Span::styled(call.tool_name.clone(), Style::default().fg(accent).bold()),
                Span::styled(format!("({})", short), Style::default().fg(muted)),
            ]));
        }

        lines
    }
}

impl QueuePanel {
    /// Shared text builder for both refresh() and context()
    fn format_context_text(state: &State) -> String {
        let qs = QueueState::get(state);
        let mut text = String::new();
        if !qs.active && qs.queued_calls.is_empty() {
            text.push_str("Queue inactive.\n");
        } else if qs.active && qs.queued_calls.is_empty() {
            text.push_str("Queue active — 0 actions queued.\n");
        } else {
            let status = if qs.active { "Active" } else { "Paused" };
            text.push_str(&format!("Queue {} — {} action(s) queued:\n\n", status, qs.queued_calls.len()));
            for call in &qs.queued_calls {
                let params = serde_json::to_string(&call.input).unwrap_or_default();
                let short = if params.len() > 120 {
                    let mut end = 117;
                    while !params.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &params[..end])
                } else {
                    params
                };
                text.push_str(&format!("{}. {}({})\n", call.index, call.tool_name, short));
            }
        }
        text
    }
}
