//! Autocomplete popup overlay.
//!
//! Adapter layer: renders [`Autocomplete`] IR data to ratatui widgets.
//! No direct `State` access.

use cp_render::conversation::Autocomplete;
use ratatui::prelude::{Frame, Line, Rect, Span, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::ui::theme;

use cp_base::cast::Safe as _;

/// Calculate the height needed for the autocomplete popup.
pub(crate) fn calculate_autocomplete_height(ac: &Autocomplete) -> u16 {
    let visible = ac.entries.len().to_u16();
    // matches + border chrome (2)
    (visible.saturating_add(2)).clamp(4, 12)
}

/// Render the @ autocomplete popup above the input area (bottom of content panel, growing upward).
pub(crate) fn render_autocomplete_popup(frame: &mut Frame<'_>, ac: &Autocomplete, area: Rect) {
    let popup_width = 60u16.min(area.width.saturating_sub(2));
    let popup_height = calculate_autocomplete_height(ac);

    // The input field occupies `input_visual_lines` at the bottom of the
    // conversation panel viewport. We want the popup's bottom edge to sit just above
    // the first line of the input field.
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
    if ac.entries.is_empty() {
        lines.push(Line::from(vec![Span::styled("  No matches", Style::default().fg(theme::text_muted()))]));
    } else {
        for (i, entry) in ac.entries.iter().enumerate() {
            lines.push(autocomplete_entry_line(entry, i == ac.selected_index));
        }
    }

    // Count indicator
    let dir_label = if ac.dir_prefix.is_empty() { ".".to_owned() } else { ac.dir_prefix.clone() };
    let count_text = format!(" @{} — {}/{} in {}/ ", ac.query, ac.total_matches, ac.total_matches, dir_label);

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

/// Build one autocomplete entry line: cursor marker, dir/file icon, label + `/` suffix.
fn autocomplete_entry_line(entry: &cp_render::conversation::AutocompleteEntry, is_selected: bool) -> Line<'static> {
    let cursor_marker = if is_selected { ">" } else { " " };
    let path_style =
        if is_selected { Style::default().fg(theme::accent()).bold() } else { Style::default().fg(theme::text()) };
    let suffix = if entry.is_dir { "/" } else { "" };
    let icon = if entry.is_dir { "📁 " } else { "   " };
    Line::from(vec![
        Span::styled(format!(" {cursor_marker} "), Style::default().fg(theme::accent())),
        Span::styled(icon.to_owned(), Style::default()),
        Span::styled(format!("{}{}", entry.label, suffix), path_style),
    ])
}
