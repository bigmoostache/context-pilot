use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use cp_base::actions::Action;
use cp_base::constants::SCROLL_ARROW_AMOUNT;
use cp_base::constants::theme;
use cp_base::panels::{ContextItem, Panel};
use cp_base::state::{ContextType, State, estimate_tokens};

use cp_base::types::spine::NotificationType;

pub struct SpinePanel;

/// Format a millisecond timestamp as HH:MM:SS
fn format_timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

impl SpinePanel {
    /// Format notifications for LLM context
    fn format_notifications_for_context(state: &State) -> String {
        let unprocessed: Vec<_> = state.notifications.iter().filter(|n| !n.processed).collect();
        let recent_processed: Vec<_> = state.notifications.iter().filter(|n| n.processed).rev().take(10).collect();

        let mut output = String::new();

        if !unprocessed.is_empty() {
            for n in &unprocessed {
                let ts = format_timestamp(n.timestamp_ms);
                output.push_str(&format!("[{}] {} {} — {}\n", n.id, ts, n.notification_type.label(), n.content));
            }
        } else {
            output.push_str("No unprocessed notifications.\n");
        }

        if !recent_processed.is_empty() {
            output.push_str("\n=== Recent Processed ===\n");
            for n in &recent_processed {
                let ts = format_timestamp(n.timestamp_ms);
                output.push_str(&format!("[{}] {} {} — {}\n", n.id, ts, n.notification_type.label(), n.content));
            }
        }

        // Show spine config summary
        output.push_str("\n=== Spine Config ===\n");
        output.push_str(&format!("max_tokens_auto_continue: {}\n", state.spine_config.max_tokens_auto_continue));
        output.push_str(&format!("continue_until_todos_done: {}\n", state.spine_config.continue_until_todos_done));
        output.push_str(&format!("auto_continuation_count: {}\n", state.spine_config.auto_continuation_count));
        if let Some(v) = state.spine_config.max_auto_retries {
            output.push_str(&format!("max_auto_retries: {}\n", v));
        }

        output.trim_end().to_string()
    }
}

impl Panel for SpinePanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        match key.code {
            KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
            KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
            KeyCode::PageUp => Some(Action::ScrollUp(10.0)),
            KeyCode::PageDown => Some(Action::ScrollDown(10.0)),
            _ => None,
        }
    }

    fn title(&self, _state: &State) -> String {
        "Spine".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_notifications_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type == ContextType::SPINE {
                ctx.token_count = token_count;
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_notifications_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type == ContextType::SPINE)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P9", 0));
        vec![ContextItem::new(id, "Spine", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let mut lines: Vec<Line> = Vec::new();

        // === Unprocessed Notifications ===
        let unprocessed: Vec<_> = state.notifications.iter().filter(|n| !n.processed).collect();

        if unprocessed.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "No unprocessed notifications".to_string(),
                Style::default().fg(theme::text_muted()).italic(),
            )]));
        } else {
            for n in &unprocessed {
                let type_color = notification_type_color(&n.notification_type);
                let ts = format_timestamp(n.timestamp_ms);
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", n.id), Style::default().fg(type_color).bold()),
                    Span::styled(format!("{} ", ts), Style::default().fg(theme::text_muted())),
                    Span::styled(n.notification_type.label().to_string(), Style::default().fg(type_color)),
                    Span::styled(
                        format!(" — {}", truncate_content(&n.content, 80)),
                        Style::default().fg(theme::text()),
                    ),
                ]));
            }
        }

        lines.push(Line::from(""));

        // === Recent Processed ===
        let recent_processed: Vec<_> = state.notifications.iter().filter(|n| n.processed).rev().take(10).collect();

        if !recent_processed.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("Processed ({})", recent_processed.len()),
                Style::default().fg(theme::text_muted()),
            )]));

            for n in &recent_processed {
                let type_color = notification_type_color(&n.notification_type);
                let ts = format_timestamp(n.timestamp_ms);
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", n.id), Style::default().fg(type_color)),
                    Span::styled(format!("{} ", ts), Style::default().fg(theme::text_muted())),
                    Span::styled(n.notification_type.label().to_string(), Style::default().fg(theme::text_muted())),
                    Span::styled(
                        format!(" — {}", truncate_content(&n.content, 60)),
                        Style::default().fg(theme::text_muted()),
                    ),
                ]));
            }
        }

        lines.push(Line::from(""));

        // === Config Summary ===
        lines.push(Line::from(vec![Span::styled("Config".to_string(), Style::default().fg(theme::text_secondary()))]));

        let config_items = vec![
            ("max_tokens_auto_continue", format!("{}", state.spine_config.max_tokens_auto_continue)),
            ("continue_until_todos_done", format!("{}", state.spine_config.continue_until_todos_done)),
            ("auto_continuations", format!("{}", state.spine_config.auto_continuation_count)),
        ];

        for (key, val) in config_items {
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", key), Style::default().fg(theme::text_muted())),
                Span::styled(": ".to_string(), Style::default().fg(theme::text_muted())),
                Span::styled(val, Style::default().fg(theme::text())),
            ]));
        }

        lines
    }
}

fn notification_type_color(nt: &NotificationType) -> Color {
    match nt {
        NotificationType::UserMessage => theme::user(),
        NotificationType::MaxTokensTruncated => theme::warning(),
        NotificationType::TodoIncomplete => theme::accent(),
        NotificationType::ReloadResume => theme::text_secondary(),
        NotificationType::Custom => theme::text_secondary(),
    }
}

/// Truncate content for display, appending "..." if truncated
fn truncate_content(s: &str, max_chars: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() > max_chars { format!("{}...", &first_line[..max_chars]) } else { first_line.to_string() }
}
