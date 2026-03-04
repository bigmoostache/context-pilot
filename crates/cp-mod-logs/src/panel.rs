use ratatui::prelude::*;

use cp_base::config::theme;
use cp_base::panels::{ContextItem, Panel};
use cp_base::state::{ContextType, State, estimate_tokens};

use crate::types::{LogEntry, LogsState};

/// Fixed panel for timestamped log entries with tree-structured summaries.
/// Un-deletable, always present when the logs module is active.
pub(crate) struct LogsPanel;

impl LogsPanel {
    /// Build the text representation used for both LLM context and UI content.
    /// Shows tree structure: top-level logs, summaries with collapse/expand,
    /// and indented children when expanded.
    pub(crate) fn format_logs_tree(state: &State) -> String {
        let ls = LogsState::get(state);
        if ls.logs.is_empty() {
            return "No logs".to_string();
        }

        let mut output = String::new();
        // Only show top-level logs (no parent_id)
        let top_level: Vec<&LogEntry> = ls.logs.iter().filter(|l| l.is_top_level()).collect();

        for log in &top_level {
            format_log_entry(&mut output, log, &ls.logs, &ls.open_log_ids, 0);
        }
        output.trim_end().to_string()
    }
}

/// Recursively format a log entry with indentation for tree display.
fn format_log_entry(output: &mut String, entry: &LogEntry, all_logs: &[LogEntry], open_ids: &[String], depth: usize) {
    let indent = "  ".repeat(depth);
    let time_str = format_timestamp(entry.timestamp_ms);

    if entry.is_summary() {
        let is_open = open_ids.contains(&entry.id);
        let icon = if is_open { "▼" } else { "▶" };
        let child_count = entry.children_ids.len();
        if is_open {
            output.push_str(&format!("{}{} [{}] {} {}\n", indent, icon, entry.id, time_str, entry.content));
            // Show children indented
            for child_id in &entry.children_ids {
                if let Some(child) = all_logs.iter().find(|l| l.id == *child_id) {
                    format_log_entry(output, child, all_logs, open_ids, depth + 1);
                }
            }
        } else {
            output.push_str(&format!(
                "{}{} [{}] {} {} ({} children)\n",
                indent, icon, entry.id, time_str, entry.content, child_count
            ));
        }
    } else {
        output.push_str(&format!("{}[{}] {} {}\n", indent, entry.id, time_str, entry.content));
    }
}

impl Panel for LogsPanel {
    fn title(&self, _state: &State) -> String {
        "Logs".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_logs_tree(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type == ContextType::LOGS {
                ctx.token_count = token_count;
                cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_logs_tree(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type == ContextType::LOGS)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P10", 0));
        vec![ContextItem::new(id, "Logs", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let ls = LogsState::get(state);
        if ls.logs.is_empty() {
            return vec![Line::from(vec![Span::styled(
                "No logs yet".to_string(),
                Style::default().fg(theme::text_muted()).italic(),
            )])];
        }

        let mut lines = Vec::new();
        let top_level: Vec<&LogEntry> = ls.logs.iter().filter(|l| l.is_top_level()).collect();

        for log in &top_level {
            render_log_entry(&mut lines, log, &ls.logs, &ls.open_log_ids, 0);
        }
        lines
    }
}

/// Recursively render a log entry as styled TUI lines.
fn render_log_entry(
    lines: &mut Vec<Line<'static>>,
    entry: &LogEntry,
    all_logs: &[LogEntry],
    open_ids: &[String],
    depth: usize,
) {
    let indent = "  ".repeat(depth);
    let time_str = format_timestamp(entry.timestamp_ms);

    if entry.is_summary() {
        let is_open = open_ids.contains(&entry.id);
        let icon = if is_open { "▼" } else { "▶" };
        let child_count = entry.children_ids.len();

        let mut spans = vec![
            Span::styled(indent, Style::default()),
            Span::styled(format!("{} ", icon), Style::default().fg(theme::accent())),
            Span::styled(format!("{} ", entry.id), Style::default().fg(theme::accent_dim())),
            Span::styled(format!("{} ", time_str), Style::default().fg(theme::text_muted())),
            Span::styled(entry.content.clone(), Style::default().fg(theme::text())),
        ];

        if !is_open {
            spans.push(Span::styled(format!(" ({} children)", child_count), Style::default().fg(theme::text_muted())));
        }

        lines.push(Line::from(spans));

        if is_open {
            for child_id in &entry.children_ids {
                if let Some(child) = all_logs.iter().find(|l| l.id == *child_id) {
                    render_log_entry(lines, child, all_logs, open_ids, depth + 1);
                }
            }
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(indent, Style::default()),
            Span::styled(format!("{} ", entry.id), Style::default().fg(theme::accent_dim())),
            Span::styled(format!("{} ", time_str), Style::default().fg(theme::text_muted())),
            Span::styled(entry.content.clone(), Style::default().fg(theme::text())),
        ]));
    }
}

fn format_timestamp(ms: u64) -> String {
    use chrono::{Local, TimeZone};
    let secs = (ms / 1000) as i64;
    Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| format!("{}ms", ms))
}
