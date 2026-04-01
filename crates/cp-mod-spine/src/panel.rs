use crossterm::event::KeyEvent;
use ratatui::prelude::{Color, Line, Span, Style};
use unicode_width::UnicodeWidthStr;

use cp_base::config::accessors::theme;
use cp_base::panels::{ContextItem, Panel, now_ms, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::state::watchers::WatcherRegistry;

use crate::types::{NotificationType, SpineState};
use std::fmt::Write as _;

/// Panel for displaying spine notifications, watchers, and config.
pub(crate) struct SpinePanel;

/// Format a millisecond timestamp as HH:MM:SS
fn format_timestamp(ms: u64) -> String {
    let secs = cp_base::panels::time_arith::ms_to_secs(ms);
    let (hours, minutes, seconds) = cp_base::panels::time_arith::secs_to_hms(secs);
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
                let _r = writeln!(output, "[{}] {} {} — {}", n.id, ts, n.kind.label(), n.content);
            }
        }

        if !blocked.is_empty() {
            output.push_str("\n=== Blocked (awaiting guard rail clearance) ===\n");
            for n in &blocked {
                let ts = format_timestamp(n.timestamp_ms);
                let _r = writeln!(output, "[{}] {} {} — {}", n.id, ts, n.kind.label(), n.content);
            }
        }

        if !recent_processed.is_empty() {
            output.push_str("\n=== Recent Processed ===\n");
            for n in &recent_processed {
                let ts = format_timestamp(n.timestamp_ms);
                let _r = writeln!(output, "[{}] {} {} — {}", n.id, ts, n.kind.label(), n.content);
            }
        }

        // Show spine config summary
        output.push_str("\n=== Spine Config ===\n");
        let _r1 =
            writeln!(output, "continue_until_todos_done: {}", SpineState::get(state).config.continue_until_todos_done);
        let _r2 =
            writeln!(output, "auto_continuation_count: {}", SpineState::get(state).config.auto_continuation_count);
        if let Some(v) = SpineState::get(state).config.max_auto_retries {
            let _r3 = writeln!(output, "max_auto_retries: {v}");
        }

        // Show active watchers
        if let Some(registry) = state.get_ext::<WatcherRegistry>() {
            let watchers = registry.active_watchers();
            if !watchers.is_empty() {
                output.push_str("\n=== Active Watchers ===\n");
                let now = now_ms();
                for w in watchers {
                    let age_s = cp_base::panels::time_arith::ms_to_secs(now.saturating_sub(w.registered_ms()));
                    let mode = if w.is_blocking() { "blocking" } else { "async" };
                    let _r4 = writeln!(output, "[{}] {} ({}, {}s ago)", w.id(), w.description(), mode, age_s);
                }
            }
        }

        output.trim_end().to_string()
    }
}

impl Panel for SpinePanel {
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

    fn render(&self, _frame: &mut ratatui::Frame<'_>, _state: &mut State, _area: ratatui::prelude::Rect) {}

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn title(&self, _state: &State) -> String {
        "Spine".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_notifications_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::SPINE {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        2
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_notifications_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::SPINE)
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
                let type_color = notification_type_color(n.kind);
                let ts = format_timestamp(n.timestamp_ms);
                let prefix = format!("{} {} {} — ", n.id, ts, n.kind.label());
                let prefix_width = UnicodeWidthStr::width(prefix.as_str());
                let content_max =
                    if viewport > prefix_width.saturating_add(10) { viewport.saturating_sub(prefix_width) } else { 40 };
                let wrapped = wrap_text_simple(&n.content, content_max);

                // First line with full prefix
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", n.id), Style::default().fg(type_color).bold()),
                    Span::styled(format!("{ts} "), Style::default().fg(theme::text_muted())),
                    Span::styled(n.kind.label().to_string(), Style::default().fg(type_color)),
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
                let type_color = notification_type_color(n.kind);
                let ts = format_timestamp(n.timestamp_ms);
                let prefix = format!("{} {} {} — ", n.id, ts, n.kind.label());
                let prefix_width = UnicodeWidthStr::width(prefix.as_str());
                let content_max =
                    if viewport > prefix_width.saturating_add(10) { viewport.saturating_sub(prefix_width) } else { 40 };
                let wrapped = wrap_text_simple(&n.content, content_max);

                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", n.id), Style::default().fg(type_color)),
                    Span::styled(format!("{ts} "), Style::default().fg(theme::text_muted())),
                    Span::styled(n.kind.label().to_string(), Style::default().fg(theme::warning())),
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
                let type_color = notification_type_color(n.kind);
                let ts = format_timestamp(n.timestamp_ms);
                let prefix = format!("{} {} {} — ", n.id, ts, n.kind.label());
                let prefix_width = UnicodeWidthStr::width(prefix.as_str());
                let content_max =
                    if viewport > prefix_width.saturating_add(10) { viewport.saturating_sub(prefix_width) } else { 40 };
                let wrapped = wrap_text_simple(&n.content, content_max);

                // First line with full prefix
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", n.id), Style::default().fg(type_color)),
                    Span::styled(format!("{ts} "), Style::default().fg(theme::text_muted())),
                    Span::styled(n.kind.label().to_string(), Style::default().fg(theme::text_muted())),
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
                    let age_s = cp_base::panels::time_arith::ms_to_secs(now.saturating_sub(w.registered_ms()));
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

/// Map a notification type to its display color.
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
        } else if current_width.saturating_add(1).saturating_add(word_width) <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
            current_width = current_width.saturating_add(1).saturating_add(word_width);
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
