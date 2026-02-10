use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::core::panels::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::SCROLL_ARROW_AMOUNT;
use crate::state::{estimate_tokens, ContextType, State};
use crate::ui::theme;

use super::types::NotificationType;

pub struct SpinePanel;

impl SpinePanel {
    /// Format notifications for LLM context
    fn format_notifications_for_context(state: &State) -> String {
        let unprocessed: Vec<_> = state.notifications.iter().filter(|n| !n.processed).collect();
        let recent_processed: Vec<_> = state.notifications.iter()
            .filter(|n| n.processed)
            .rev()
            .take(10)
            .collect();

        let mut output = String::new();

        if !unprocessed.is_empty() {
            output.push_str("=== Unprocessed Notifications ===\n");
            for n in &unprocessed {
                output.push_str(&format!("[{}] {} — {} (source: {})\n",
                    n.id, n.notification_type.label(), n.content, n.source));
            }
        } else {
            output.push_str("No unprocessed notifications.\n");
        }

        if !recent_processed.is_empty() {
            output.push_str("\n=== Recent Processed ===\n");
            for n in &recent_processed {
                output.push_str(&format!("[{}] {} — {}\n",
                    n.id, n.notification_type.label(), n.content));
            }
        }

        // Show spine config summary
        output.push_str(&format!("\n=== Spine Config ===\n"));
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
            if ctx.context_type == ContextType::Spine {
                ctx.token_count = token_count;
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_notifications_for_context(state);
        let (id, last_refresh_ms) = state.context.iter()
            .find(|c| c.context_type == ContextType::Spine)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P9", 0));
        vec![ContextItem::new(id, "Spine", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut lines: Vec<Line> = Vec::new();

        // === Unprocessed Notifications ===
        let unprocessed: Vec<_> = state.notifications.iter().filter(|n| !n.processed).collect();

        lines.push(Line::from(vec![
            Span::styled(" ▸ ", Style::default().fg(theme::accent()).bold()),
            Span::styled("Unprocessed Notifications", Style::default().fg(theme::text()).bold()),
            Span::styled(format!(" ({})", unprocessed.len()), Style::default().fg(theme::text_muted())),
        ]));

        if unprocessed.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("   ", base_style),
                Span::styled("None", Style::default().fg(theme::text_muted()).italic()),
            ]));
        } else {
            for n in &unprocessed {
                let type_color = match n.notification_type {
                    NotificationType::UserMessage => theme::user(),
                    NotificationType::MaxTokensTruncated => theme::warning(),
                    NotificationType::TodoIncomplete => theme::accent(),
                    NotificationType::Custom => theme::text_secondary(),
                };
                lines.push(Line::from(vec![
                    Span::styled("   ", base_style),
                    Span::styled(format!("{}", n.id), Style::default().fg(theme::accent_dim())),
                    Span::styled(" ", base_style),
                    Span::styled(n.notification_type.label().to_string(), Style::default().fg(type_color).bold()),
                    Span::styled(format!(" — {}", n.content), Style::default().fg(theme::text())),
                ]));
            }
        }

        lines.push(Line::from(""));

        // === Recent Processed ===
        let recent_processed: Vec<_> = state.notifications.iter()
            .filter(|n| n.processed)
            .rev()
            .take(10)
            .collect();

        lines.push(Line::from(vec![
            Span::styled(" ▸ ", Style::default().fg(theme::text_muted())),
            Span::styled("Recent Processed", Style::default().fg(theme::text_secondary())),
            Span::styled(format!(" ({})", recent_processed.len()), Style::default().fg(theme::text_muted())),
        ]));

        if recent_processed.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("   ", base_style),
                Span::styled("None", Style::default().fg(theme::text_muted()).italic()),
            ]));
        } else {
            for n in &recent_processed {
                lines.push(Line::from(vec![
                    Span::styled("   ", base_style),
                    Span::styled(format!("{}", n.id), Style::default().fg(theme::text_muted())),
                    Span::styled(" ", base_style),
                    Span::styled(n.notification_type.label().to_string(), Style::default().fg(theme::text_muted())),
                    Span::styled(format!(" — {}", n.content), Style::default().fg(theme::text_muted())),
                ]));
            }
        }

        lines.push(Line::from(""));

        // === Config Summary ===
        lines.push(Line::from(vec![
            Span::styled(" ▸ ", Style::default().fg(theme::text_muted())),
            Span::styled("Config", Style::default().fg(theme::text_secondary())),
        ]));

        let config_items = vec![
            ("max_tokens_auto_continue", format!("{}", state.spine_config.max_tokens_auto_continue)),
            ("continue_until_todos_done", format!("{}", state.spine_config.continue_until_todos_done)),
            ("auto_continuations", format!("{}", state.spine_config.auto_continuation_count)),
        ];

        for (key, val) in config_items {
            lines.push(Line::from(vec![
                Span::styled("   ", base_style),
                Span::styled(key.to_string(), Style::default().fg(theme::text_muted())),
                Span::styled(": ", Style::default().fg(theme::text_muted())),
                Span::styled(val, Style::default().fg(theme::text())),
            ]));
        }

        lines
    }
}
