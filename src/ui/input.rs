use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::state::State;
use super::{theme, spinner};

pub fn render_input(frame: &mut Frame, state: &State, area: Rect) {
    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    let is_empty = state.input.is_empty();
    // Only streaming blocks input - cleaning is passthrough
    let is_busy = state.is_streaming;

    let (title, title_color, border_color) = if state.is_streaming {
        let spin = spinner::spinner(state.spinner_frame);
        (format!(" {} Streaming... ", spin), theme::SUCCESS, theme::SUCCESS)
    } else {
        (" Message ".to_string(), theme::ACCENT, theme::BORDER_FOCUS)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme::BG_INPUT))
        .title(Span::styled(&title, Style::default().fg(title_color)));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    // Input content or placeholder
    let content = if is_empty && !is_busy {
        vec![Line::from(vec![
            Span::styled(" Type your message here...", Style::default().fg(theme::TEXT_MUTED).italic()),
        ])]
    } else {
        state.input.split('\n')
            .map(|line| Line::from(vec![
                Span::styled(" ", Style::default()),
                Span::styled(line, Style::default().fg(theme::TEXT)),
            ]))
            .collect()
    };

    let paragraph = Paragraph::new(content)
        .style(Style::default().bg(theme::BG_INPUT));

    frame.render_widget(paragraph, content_area);

    // Cursor positioning - only blocked during streaming
    if !is_busy && !is_empty {
        let before_cursor = &state.input[..state.input_cursor];
        let line_num = before_cursor.matches('\n').count();
        let line_start = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = state.input_cursor - line_start;

        frame.set_cursor_position(Position::new(
            content_area.x + col as u16 + 1,
            content_area.y + line_num as u16,
        ));
    } else if !is_busy {
        // Cursor at start for empty input
        frame.set_cursor_position(Position::new(
            content_area.x + 1,
            content_area.y,
        ));
    }
}

pub fn render_status_bar(frame: &mut Frame, state: &State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_BASE).fg(theme::TEXT_MUTED);
    let spin = spinner::spinner(state.spinner_frame);

    let mut spans = vec![
        Span::styled(" ", base_style),
    ];

    // Show all active states as separate badges with spinners
    if state.is_streaming {
        spans.push(Span::styled(
            format!(" {} STREAMING ", spin),
            Style::default().fg(theme::BG_BASE).bg(theme::SUCCESS).bold()
        ));
        spans.push(Span::styled(" ", base_style));
    }

    if state.is_cleaning_context {
        spans.push(Span::styled(
            format!(" {} CLEANING ", spin),
            Style::default().fg(theme::BG_BASE).bg(theme::ACCENT).bold()
        ));
        spans.push(Span::styled(" ", base_style));
    }

    if state.pending_tldrs > 0 {
        spans.push(Span::styled(
            format!(" {} SUMMARIZING {} ", spin, state.pending_tldrs),
            Style::default().fg(theme::BG_BASE).bg(theme::WARNING).bold()
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // Count loading context elements (those without cached content)
    let loading_count = state.context.iter()
        .filter(|c| c.cached_content.is_none() && c.context_type.needs_cache())
        .count();

    if loading_count > 0 {
        spans.push(Span::styled(
            format!(" {} LOADING {} ", spin, loading_count),
            Style::default().fg(theme::BG_BASE).bg(theme::TEXT_MUTED).bold()
        ));
        spans.push(Span::styled(" ", base_style));
    }

    // If nothing active, show READY
    if !state.is_streaming && !state.is_cleaning_context && state.pending_tldrs == 0 && loading_count == 0 {
        spans.push(Span::styled(" READY ", Style::default().fg(theme::BG_BASE).bg(theme::TEXT_MUTED).bold()));
        spans.push(Span::styled(" ", base_style));
    }

    // Right side info
    let char_count = state.input.chars().count();
    let right_info = if char_count > 0 {
        format!("{} chars ", char_count)
    } else {
        String::new()
    };

    let left_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_width = right_info.len();
    let padding = (area.width as usize).saturating_sub(left_width + right_width);

    spans.push(Span::styled(" ".repeat(padding), base_style));
    spans.push(Span::styled(&right_info, base_style));

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}
