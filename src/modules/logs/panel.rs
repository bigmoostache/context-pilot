use ratatui::prelude::*;

use crate::core::panels::{ContextItem, Panel};
use crate::state::{estimate_tokens, ContextType, State};
use crate::ui::theme;

/// Fixed panel for timestamped log entries.
/// Un-deletable, always present when the logs module is active.
pub struct LogsPanel;

impl LogsPanel {
    fn format_logs_for_context(state: &State) -> String {
        if state.logs.is_empty() {
            return "No logs".to_string();
        }

        let mut output = String::new();
        for entry in &state.logs {
            let time_str = format_timestamp(entry.timestamp_ms);
            output.push_str(&format!("[{}] {} {}\n", entry.id, time_str, entry.content));
        }
        output.trim_end().to_string()
    }
}

impl Panel for LogsPanel {
    fn title(&self, _state: &State) -> String {
        "Logs".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_logs_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type == ContextType::Logs {
                ctx.token_count = token_count;
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_logs_for_context(state);
        let (id, last_refresh_ms) = state.context.iter()
            .find(|c| c.context_type == ContextType::Logs)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P10", 0));
        vec![ContextItem::new(id, "Logs", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        if state.logs.is_empty() {
            return vec![Line::from(vec![
                Span::styled(
                    "No logs yet".to_string(),
                    Style::default().fg(theme::text_muted()).italic(),
                ),
            ])];
        }

        let mut lines = Vec::new();
        for entry in &state.logs {
            let time_str = format_timestamp(entry.timestamp_ms);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", entry.id),
                    Style::default().fg(theme::accent_dim()),
                ),
                Span::styled(
                    format!("{} ", time_str),
                    Style::default().fg(theme::text_muted()),
                ),
                Span::styled(
                    entry.content.clone(),
                    Style::default().fg(theme::text()),
                ),
            ]));
        }
        lines
    }
}

fn format_timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
