//! Threads view — dedicated TUI layout when `ViewMode::Threads` is active.
//!
//! Renders a two-pane layout: thread list (left) + message area (right).
//! Completely replaces the standard sidebar + panel view.

use ratatui::Frame;
use ratatui::prelude::{Constraint, Direction, Layout, Rect, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::state::State;
use crate::ui::theme;
use cp_base::cast::Safe as _;
use cp_mod_threads::types::{FocusState, ThreadStatus, ThreadsState};

/// Width of the thread list pane in columns.
const THREAD_LIST_WIDTH: u16 = 28;

/// Render the threads view: thread list + message area.
pub(crate) fn render_threads_view(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let threads_state = ThreadsState::get(state);
    let focus_state = FocusState::get(state);

    if threads_state.threads.is_empty() {
        render_empty_state(frame, area);
        return;
    }

    // Clamp selected index
    let selected_idx = focus_state.selected_thread_idx.min(threads_state.threads.len().saturating_sub(1));

    // Two-pane layout: thread list | message area
    if area.width > THREAD_LIST_WIDTH.saturating_add(20) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(THREAD_LIST_WIDTH), Constraint::Min(1)])
            .split(area);

        let (Some(&list_area), Some(&msg_area)) = (layout.first(), layout.get(1)) else { return };
        render_thread_list(frame, threads_state, selected_idx, list_area);
        render_message_area(frame, threads_state, selected_idx, msg_area);
    } else {
        // Narrow terminal — show thread list only
        render_thread_list(frame, threads_state, selected_idx, area);
    }
}

/// Render the "no threads" empty state with a helpful message.
fn render_empty_state(frame: &mut Frame<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::border_muted()))
        .title(Span::styled(" Threads ", Style::default().fg(theme::accent())));

    let text = vec![
        Line::from(""),
        Line::from(Span::styled("No threads yet.", Style::default().fg(theme::text_muted()))),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(theme::text_muted())),
            Span::styled("Ctrl+V", Style::default().fg(theme::accent())),
            Span::styled(" to return to panel view.", Style::default().fg(theme::text_muted())),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block).alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(paragraph, area);
}

/// Render the left-pane thread list with selection indicator.
fn render_thread_list(frame: &mut Frame<'_>, ts: &ThreadsState, selected: usize, area: Rect) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(theme::border_muted()))
        .style(Style::default().bg(theme::bg_base()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build lines for each thread
    let mut lines: Vec<Line<'_>> = Vec::new();

    for (i, thread) in ts.threads.iter().enumerate() {
        let is_selected = i == selected;

        // Indicator + thread name
        let indicator = if is_selected { "▸ " } else { "  " };
        let name_color = if is_selected { theme::accent() } else { theme::text() };
        let name = truncate_str(&thread.name, inner.width.saturating_sub(4).into());

        lines.push(Line::from(vec![
            Span::styled(indicator, Style::default().fg(theme::accent())),
            Span::styled(name, Style::default().fg(name_color)),
        ]));

        // Status badge
        let (badge, badge_color) = match thread.status {
            ThreadStatus::MyTurn => ("[MY_TURN]", theme::accent()),
            ThreadStatus::TheirTurn => ("[THEIR_TURN]", theme::text_muted()),
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(badge, Style::default().fg(badge_color)),
            Span::styled(
                format!("  {} msg", thread.messages.len()),
                Style::default().fg(theme::text_muted()),
            ),
        ]));

        // Separator between threads
        if i < ts.threads.len().saturating_sub(1) {
            lines.push(Line::from(""));
        }
    }

    // Help hints at the bottom
    let help_y = inner.height.saturating_sub(1);
    if help_y > 0 && inner.height > lines.len().to_u16() {
        // Pad to push help to bottom
        let needed = (help_y as usize).saturating_sub(lines.len());
        for _ in 0..needed {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(vec![
            Span::styled(" Ctrl+V", Style::default().fg(theme::accent())),
            Span::styled(" view", Style::default().fg(theme::text_muted())),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the right-pane message area for the selected thread.
fn render_message_area(frame: &mut Frame<'_>, ts: &ThreadsState, selected: usize, area: Rect) {
    let Some(thread) = ts.threads.get(selected) else {
        return;
    };

    // Title: thread name + status
    let (status_label, status_color) = match thread.status {
        ThreadStatus::MyTurn => (" [MY_TURN]", theme::accent()),
        ThreadStatus::TheirTurn => (" [THEIR_TURN]", theme::text_muted()),
    };

    let title = Line::from(vec![
        Span::styled(format!(" {} ", thread.name), Style::default().fg(theme::text())),
        Span::styled(status_label, Style::default().fg(status_color)),
        Span::raw(" "),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::border_muted()))
        .title(title)
        .style(Style::default().bg(theme::bg_base()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if thread.messages.is_empty() {
        let empty = Paragraph::new(Span::styled(
            "No messages in this thread.",
            Style::default().fg(theme::text_muted()),
        ));
        frame.render_widget(empty, inner);
        return;
    }

    // Build message lines
    let mut lines: Vec<Line<'_>> = Vec::new();

    for msg in &thread.messages {
        // Author + timestamp header
        let author_label = msg.author.to_string();
        let author_color = match msg.author {
            cp_mod_threads::types::ThreadAuthor::User => theme::accent(),
            cp_mod_threads::types::ThreadAuthor::Assistant => theme::assistant(),
        };
        let time = format_time_ms(msg.timestamp);

        lines.push(Line::from(vec![
            Span::styled(format!("[{author_label}"), Style::default().fg(author_color)),
            Span::styled(format!(" {time}]"), Style::default().fg(theme::text_muted())),
        ]));

        // Content
        if let Some(content) = &msg.content {
            for line in content.lines() {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(theme::text()),
                )));
            }
        }

        // File path reference
        if let Some(path) = &msg.file_path {
            lines.push(Line::from(Span::styled(
                format!("📎 {path}"),
                Style::default().fg(theme::accent_dim()),
            )));
        }

        // Question indicator
        if msg.question.is_some() {
            lines.push(Line::from(Span::styled(
                "❓ Question attached",
                Style::default().fg(theme::warning()),
            )));
        }

        // Blank line between messages
        lines.push(Line::from(""));
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Truncate a string to `max_len` characters, appending "…" if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let mut result: String = s.chars().take(max_len.saturating_sub(1)).collect();
        result.push('…');
        result
    }
}

/// Format a millisecond timestamp as HH:MM.
fn format_time_ms(ms: u64) -> String {
    let secs = cp_base::panels::time_arith::ms_to_secs(ms);
    let (hours, minutes, _seconds) = cp_base::panels::time_arith::secs_to_hms(secs);
    format!("{hours:02}:{minutes:02}")
}
