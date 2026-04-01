use ratatui::{
    prelude::{Frame, Line, Rect, Span, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::theme;
use crate::state::State;

use cp_base::cast::Safe as _;

/// Calculate the height needed for the question form.
pub(super) fn calculate_question_form_height(form: &cp_base::ui::question_form::PendingForm) -> u16 {
    let Some(q) = form.questions.get(form.current_question) else { return 6 };
    // Header line + question text + blank + options (including Other) + blank + nav hint
    let option_lines = q.options.len().to_u16().saturating_add(1); // +1 for "Other"
    let header_lines = 2u16; // header + question text
    let chrome = 4u16; // borders (2) + spacing + nav hint
    (header_lines.saturating_add(option_lines.saturating_mul(2)).saturating_add(chrome)).min(20) // each option: label + description
}

/// Render the question form at the bottom of the screen.
pub(super) fn render_question_form(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let Some(form) = state.get_ext::<cp_base::ui::question_form::PendingForm>() else { return };

    let q_idx = form.current_question;
    let Some(q) = form.questions.get(q_idx) else { return };
    let Some(ans) = form.answers.get(q_idx) else { return };
    let other_idx = q.options.len();

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Progress indicator
    let progress = if form.questions.len() > 1 {
        format!(" ({}/{}) ", q_idx.saturating_add(1), form.questions.len())
    } else {
        String::new()
    };

    // Question text
    lines.push(Line::from(vec![
        Span::styled(format!(" {} ", q.header), Style::default().fg(theme::bg_base()).bg(theme::accent()).bold()),
        Span::styled(format!(" {}", q.text), Style::default().fg(theme::text()).bold()),
    ]));
    lines.push(Line::from(""));

    // Options
    for (i, opt) in q.options.iter().enumerate() {
        let is_cursor = ans.cursor == i;
        let is_selected = ans.selected.contains(&i);

        let indicator = if is_selected && q.multi_select {
            "[x]"
        } else if is_selected {
            "(●)"
        } else if q.multi_select {
            "[ ]"
        } else {
            "( )"
        };

        let cursor_marker = if is_cursor { ">" } else { " " };

        let label_style = if is_cursor {
            Style::default().fg(theme::accent()).bold()
        } else if is_selected {
            Style::default().fg(theme::success()).bold()
        } else {
            Style::default().fg(theme::text())
        };

        let desc_style = if is_cursor {
            Style::default().fg(theme::text_secondary())
        } else {
            Style::default().fg(theme::text_muted())
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {cursor_marker} "), Style::default().fg(theme::accent())),
            Span::styled(format!("{indicator} "), label_style),
            Span::styled(opt.label.clone(), label_style),
            Span::styled(format!("  {}", opt.description), desc_style),
        ]));
    }

    // "Other" option
    {
        let is_cursor = ans.cursor == other_idx;
        let is_typing = ans.typing_other;

        let cursor_marker = if is_cursor { ">" } else { " " };
        let indicator = if is_typing { "(●)" } else { "( )" };

        let label_style = if is_cursor {
            Style::default().fg(theme::accent()).bold()
        } else if is_typing {
            Style::default().fg(theme::success()).bold()
        } else {
            Style::default().fg(theme::text())
        };

        if is_typing {
            lines.push(Line::from(vec![
                Span::styled(format!(" {cursor_marker} "), Style::default().fg(theme::accent())),
                Span::styled(format!("{indicator} "), label_style),
                Span::styled("Other: ", label_style),
                Span::styled(
                    format!("{}▏", ans.other_text),
                    Style::default().fg(theme::text()).bg(theme::bg_elevated()),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!(" {cursor_marker} "), Style::default().fg(theme::accent())),
                Span::styled(format!("{indicator} "), label_style),
                Span::styled("Other", label_style),
                Span::styled("  Type your own answer", Style::default().fg(theme::text_muted())),
            ]));
        }
    }

    // Navigation hint
    lines.push(Line::from(""));
    let hint_spans = if q.multi_select {
        vec![
            Span::styled(" ↑↓", Style::default().fg(theme::accent())),
            Span::styled(" navigate  ", Style::default().fg(theme::text_muted())),
            Span::styled("←→", Style::default().fg(theme::accent())),
            Span::styled(" questions  ", Style::default().fg(theme::text_muted())),
            Span::styled("Space", Style::default().fg(theme::accent())),
            Span::styled(" toggle  ", Style::default().fg(theme::text_muted())),
            Span::styled("Enter", Style::default().fg(theme::accent())),
            Span::styled(" confirm  ", Style::default().fg(theme::text_muted())),
            Span::styled("Esc", Style::default().fg(theme::accent())),
            Span::styled(" dismiss", Style::default().fg(theme::text_muted())),
        ]
    } else {
        vec![
            Span::styled(" ↑↓", Style::default().fg(theme::accent())),
            Span::styled(" navigate  ", Style::default().fg(theme::text_muted())),
            Span::styled("←→", Style::default().fg(theme::accent())),
            Span::styled(" questions  ", Style::default().fg(theme::text_muted())),
            Span::styled("Enter", Style::default().fg(theme::accent())),
            Span::styled(" select & next  ", Style::default().fg(theme::text_muted())),
            Span::styled("Esc", Style::default().fg(theme::accent())),
            Span::styled(" dismiss", Style::default().fg(theme::text_muted())),
        ]
    };
    lines.push(Line::from(hint_spans));

    let title = format!(" Question{progress} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent()))
        .style(Style::default().bg(theme::bg_surface()))
        .title(Span::styled(title, Style::default().fg(theme::accent()).bold()));

    let paragraph = Paragraph::new(lines).block(block).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Calculate the height needed for the autocomplete popup.
pub(super) fn calculate_autocomplete_height(ac: &cp_base::state::autocomplete::Suggestions) -> u16 {
    let visible = ac.visible_matches().len().to_u16();
    // matches + border chrome (2)
    (visible.saturating_add(2)).clamp(4, 12)
}

/// Render the @ autocomplete popup above the input area (bottom of content panel, growing upward).
pub(super) fn render_autocomplete_popup(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let ac = match state.get_ext::<cp_base::state::autocomplete::Suggestions>() {
        Some(ac) if ac.active => ac,
        _ => return,
    };

    let popup_width = 60u16.min(area.width.saturating_sub(2));
    let popup_height = calculate_autocomplete_height(ac);

    // The input field (🦊 ...) occupies `input_visual_lines` at the bottom of the
    // conversation panel viewport. We want the popup's bottom edge to sit just above
    // the first line of the input field.
    //
    // area = the content region (right of sidebar, above status bar).
    // The conversation panel fills this area with a 1-cell border on each side,
    // so usable inner height = area.height - 2 (top/bottom border).
    // The input starts at: area.bottom() - 1 (bottom border) - input_visual_lines
    // We place the popup bottom at that position.
    let border_chrome = 2u16; // top + bottom border of the conversation panel
    let input_lines = ac.input_visual_lines;
    let scroll_padding = 2u16; // padding lines below input in the conversation panel
    let popup_bottom = area.y.saturating_add(
        area.height.saturating_sub(border_chrome.saturating_add(input_lines).saturating_add(scroll_padding)),
    );
    let popup_top = popup_bottom.saturating_sub(popup_height);
    // Clamp: don't go above the top of the content area (+1 for border)
    let y = popup_top.max(area.y.saturating_add(1));
    let clamped_height = popup_bottom.saturating_sub(y);
    if clamped_height < 3 {
        return; // Not enough space to render
    }

    let x = area.x.saturating_add(1); // +1 to clear the panel's left border
    let popup_area = Rect::new(x, y, popup_width, clamped_height);

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Show matches
    let visible = ac.visible_matches();
    if visible.is_empty() {
        lines.push(Line::from(vec![Span::styled("  No matches", Style::default().fg(theme::text_muted()))]));
    } else {
        for (i, entry) in visible.iter().enumerate() {
            let abs_idx = ac.scroll_offset.saturating_add(i);
            let is_selected = abs_idx == ac.selected;

            let cursor_marker = if is_selected { ">" } else { " " };
            let path_style = if is_selected {
                Style::default().fg(theme::accent()).bold()
            } else {
                Style::default().fg(theme::text())
            };

            let suffix = if entry.is_dir { "/" } else { "" };
            let icon = if entry.is_dir { "📁 " } else { "   " };
            lines.push(Line::from(vec![
                Span::styled(format!(" {cursor_marker} "), Style::default().fg(theme::accent())),
                Span::styled(icon.to_string(), Style::default()),
                Span::styled(format!("{}{}", entry.name, suffix), path_style),
            ]));
        }
    }

    // Count indicator
    let dir_label = if ac.dir_prefix.is_empty() { ".".to_string() } else { ac.dir_prefix.clone() };
    let count_text = format!(" @{} — {}/{} in {}/ ", ac.query, ac.matches.len(), ac.matches.len(), dir_label);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent()))
        .style(Style::default().bg(theme::bg_surface()))
        .title(Span::styled(count_text, Style::default().fg(theme::accent()).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup_area);
    frame.render_widget(paragraph, popup_area);
}
