use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use cp_base::config::constants::SCROLL_ARROW_AMOUNT;
use cp_base::config::theme;
use cp_base::panels::{ContextItem, Panel, now_ms};
use cp_base::state::Action;
use cp_base::state::{ContextType, State, estimate_tokens};
use cp_base::watchers::WatcherRegistry;

use crate::types::{NotificationType, SpineState};

pub(crate) struct SpinePanel;

/// Format a millisecond timestamp as HH:MM:SS
fn format_timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

impl SpinePanel {
    /// Format notifications for LLM context
    fn format_notifications_for_context(state: &State) -> String {
        let unprocessed: Vec<_> = SpineState::get(state).notifications.iter().filter(|n| n.is_unprocessed()).collect();
        let blocked: Vec<_> = SpineState::get(state)
            .notifications
            .iter()
            .filter(|n| n.status == crate::types::NotificationStatus::Blocked)
            .collect();
        let recent_processed: Vec<_> =
            SpineState::get(state).notifications.iter().filter(|n| n.is_processed()).rev().take(10).collect();

        let mut output = String::new();

        if unprocessed.is_empty() {
            output.push_str("No unprocessed notifications.\n");
        } else {
            for n in &unprocessed {
                let ts = format_timestamp(n.timestamp_ms);
                output.push_str(&format!("[{}] {} {} — {}\n", n.id, ts, n.notification_type.label(), n.content));
            }
        }

        if !blocked.is_empty() {
            output.push_str("\n=== Blocked (awaiting guard rail clearance) ===\n");
            for n in &blocked {
                let ts = format_timestamp(n.timestamp_ms);
                output.push_str(&format!("[{}] {} {} — {}\n", n.id, ts, n.notification_type.label(), n.content));
            }
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
        output.push_str(&format!(
            "continue_until_todos_done: {}\n",
            SpineState::get(state).config.continue_until_todos_done
        ));
        output
            .push_str(&format!("auto_continuation_count: {}\n", SpineState::get(state).config.auto_continuation_count));
        if let Some(v) = SpineState::get(state).config.max_auto_retries {
            output.push_str(&format!("max_auto_retries: {v}\n"));
        }

        // Show active watchers
        if let Some(registry) = state.get_ext::<WatcherRegistry>() {
            let watchers = registry.active_watchers();
            if !watchers.is_empty() {
                output.push_str("\n=== Active Watchers ===\n");
                let now = now_ms();
                for w in watchers {
                    let age_s = (now.saturating_sub(w.registered_ms())) / 1000;
                    let mode = if w.is_blocking() { "blocking" } else { "async" };
                    output.push_str(&format!("[{}] {} ({}, {}s ago)\n", w.id(), w.description(), mode, age_s));
                }
            }
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
                let _ = cp_base::panels::update_if_changed(ctx, &content);
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
            .map_or(("P9", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Spine", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'_>> = Vec::new();

        // === Unprocessed Notifications ===
        let unprocessed: Vec<_> = SpineState::get(state).notifications.iter().filter(|n| n.is_unprocessed()).collect();

        if unprocessed.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "No unprocessed notifications".to_string(),
                Style::default().fg(theme::text_muted()).italic(),
            )]));
        } else {
            // Calculate wrap width: viewport minus the prefix "N999 HH:MM:SS TYPE — "
            let viewport = state.last_viewport_width as usize;
            for n in &unprocessed {
                let type_color = notification_type_color(n.notification_type);
                let ts = format_timestamp(n.timestamp_ms);
                let prefix = format!("{} {} {} — ", n.id, ts, n.notification_type.label());
                let prefix_width = UnicodeWidthStr::width(prefix.as_str());
                let content_max = if viewport > prefix_width + 10 { viewport - prefix_width } else { 40 };
                let wrapped = wrap_text_simple(&n.content, content_max);

                // First line with full prefix
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", n.id), Style::default().fg(type_color).bold()),
                    Span::styled(format!("{ts} "), Style::default().fg(theme::text_muted())),
                    Span::styled(n.notification_type.label().to_string(), Style::default().fg(type_color)),
                    Span::styled(
                        format!(" — {}", wrapped.first().map_or("", |s| s.as_str())),
                        Style::default().fg(theme::text()),
                    ),
                ]));
                // Continuation lines indented to align with content
                for line in wrapped.iter().skip(1) {
                    let indent = " ".repeat(prefix_width);
                    lines.push(Line::from(vec![
                        Span::styled(indent, Style::default()),
                        Span::styled(line.clone(), Style::default().fg(theme::text())),
                    ]));
                }
            }
        }

        lines.push(Line::from(""));

        // === Blocked Notifications (held by guard rails) ===
        let blocked: Vec<_> = SpineState::get(state)
            .notifications
            .iter()
            .filter(|n| n.status == crate::types::NotificationStatus::Blocked)
            .collect();

        if !blocked.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("Blocked ({})", blocked.len()),
                Style::default().fg(theme::warning()),
            )]));

            let viewport = state.last_viewport_width as usize;
            for n in &blocked {
                let type_color = notification_type_color(n.notification_type);
                let ts = format_timestamp(n.timestamp_ms);
                let prefix = format!("{} {} {} — ", n.id, ts, n.notification_type.label());
                let prefix_width = UnicodeWidthStr::width(prefix.as_str());
                let content_max = if viewport > prefix_width + 10 { viewport - prefix_width } else { 40 };
                let wrapped = wrap_text_simple(&n.content, content_max);

                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", n.id), Style::default().fg(type_color)),
                    Span::styled(format!("{ts} "), Style::default().fg(theme::text_muted())),
                    Span::styled(n.notification_type.label().to_string(), Style::default().fg(theme::warning())),
                    Span::styled(
                        format!(" — {}", wrapped.first().map_or("", |s| s.as_str())),
                        Style::default().fg(theme::text_muted()),
                    ),
                ]));
                for line in wrapped.iter().skip(1) {
                    let indent = " ".repeat(prefix_width);
                    lines.push(Line::from(vec![
                        Span::styled(indent, Style::default()),
                        Span::styled(line.clone(), Style::default().fg(theme::text_muted())),
                    ]));
                }
            }

            lines.push(Line::from(""));
        }

        // === Recent Processed ===
        let recent_processed: Vec<_> =
            SpineState::get(state).notifications.iter().filter(|n| n.is_processed()).rev().take(10).collect();

        if !recent_processed.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("Processed ({})", recent_processed.len()),
                Style::default().fg(theme::text_muted()),
            )]));

            let viewport = state.last_viewport_width as usize;
            for n in &recent_processed {
                let type_color = notification_type_color(n.notification_type);
                let ts = format_timestamp(n.timestamp_ms);
                let prefix = format!("{} {} {} — ", n.id, ts, n.notification_type.label());
                let prefix_width = UnicodeWidthStr::width(prefix.as_str());
                let content_max = if viewport > prefix_width + 10 { viewport - prefix_width } else { 40 };
                let wrapped = wrap_text_simple(&n.content, content_max);

                // First line with full prefix
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", n.id), Style::default().fg(type_color)),
                    Span::styled(format!("{ts} "), Style::default().fg(theme::text_muted())),
                    Span::styled(n.notification_type.label().to_string(), Style::default().fg(theme::text_muted())),
                    Span::styled(
                        format!(" — {}", wrapped.first().map_or("", |s| s.as_str())),
                        Style::default().fg(theme::text_muted()),
                    ),
                ]));
                // Continuation lines indented
                for line in wrapped.iter().skip(1) {
                    let indent = " ".repeat(prefix_width);
                    lines.push(Line::from(vec![
                        Span::styled(indent, Style::default()),
                        Span::styled(line.clone(), Style::default().fg(theme::text_muted())),
                    ]));
                }
            }
        }

        lines.push(Line::from(""));

        // === Config Summary ===
        lines.push(Line::from(vec![Span::styled("Config".to_string(), Style::default().fg(theme::text_secondary()))]));

        let config_items = vec![
            ("continue_until_todos_done", format!("{}", SpineState::get(state).config.continue_until_todos_done)),
            ("auto_continuations", format!("{}", SpineState::get(state).config.auto_continuation_count)),
        ];

        for (key, val) in config_items {
            lines.push(Line::from(vec![
                Span::styled(format!("  {key}"), Style::default().fg(theme::text_muted())),
                Span::styled(": ".to_string(), Style::default().fg(theme::text_muted())),
                Span::styled(val, Style::default().fg(theme::text())),
            ]));
        }

        // === Active Watchers ===
        if let Some(registry) = state.get_ext::<WatcherRegistry>() {
            let watchers = registry.active_watchers();
            if !watchers.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    format!("Active Watchers ({})", watchers.len()),
                    Style::default().fg(theme::accent()),
                )]));
                let now = now_ms();
                for w in watchers {
                    let age_s = (now.saturating_sub(w.registered_ms())) / 1000;
                    let mode_color = if w.is_blocking() { theme::warning() } else { theme::text_secondary() };
                    let mode_label = if w.is_blocking() { "⏳" } else { "👁" };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {mode_label} "), Style::default().fg(mode_color)),
                        Span::styled(w.description().to_string(), Style::default().fg(theme::text())),
                        Span::styled(format!(" ({age_s}s)"), Style::default().fg(theme::text_muted())),
                    ]));
                }
            }
        }

        lines
    }
}

fn notification_type_color(nt: NotificationType) -> Color {
    match nt {
        NotificationType::UserMessage => theme::user(),
        NotificationType::ReloadResume | NotificationType::Custom => theme::text_secondary(),
    }
}

/// Simple word-wrap: break text at word boundaries to fit within `max_width`.
/// Uses `UnicodeWidthStr` for correct display width measurement.
fn wrap_text_simple(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current_width == 0 {
            current_line.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(current_line);
            current_line = word.to_string();
            current_width = word_width;
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
