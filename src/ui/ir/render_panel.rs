//! Panel IR adapter — renders [`PanelContent`] to a bordered, scrollable ratatui widget.
//!
//! Replaces `panels::render_panel_default` by consuming the pre-built
//! IR snapshot for the active panel. Falls back to `Panel::content()`
//! when `PanelContent::blocks` is empty (panels not yet migrated to IR).

use cp_render::frame::PanelContent;
use ratatui::prelude::{Frame, Line, Rect, Span, Style};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::state::State;
use crate::ui::{helpers::count_wrapped_lines, theme};
use cp_base::cast::Safe as _;

/// Render the active panel from its IR snapshot.
///
/// Draws the bordered chrome (title, optional "refreshed N ago" footer),
/// converts IR blocks to ratatui lines, calculates scroll, and renders.
///
/// `text` is the pre-resolved content: either from `blocks_to_lines()`
/// or from the legacy `Panel::content()` fallback. The caller decides.
pub(crate) fn render_panel_from_ir(frame: &mut Frame<'_>, state: &mut State, area: Rect, panel_content: &PanelContent) {
    let base_style = Style::default().bg(theme::bg_surface());

    let inner_area = Rect::new(area.x.saturating_add(1), area.y, area.width.saturating_sub(2), area.height);

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::border()))
        .style(base_style)
        .title(Span::styled(format!(" {} ", panel_content.title), Style::default().fg(theme::accent()).bold()));

    if let Some(ref bottom) = panel_content.refreshed_ago {
        block = block.title_bottom(Span::styled(format!(" {bottom} "), Style::default().fg(theme::text_muted())));
    }

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    // Resolve content from IR blocks
    let text: Vec<Line<'static>> = if panel_content.blocks.is_empty() {
        // No blocks — render an empty panel
        Vec::new()
    } else {
        super::blocks_to_lines(&panel_content.blocks)
    };

    // Calculate scroll bounds from wrapped content height
    let viewport_width = content_area.width.to_usize();
    let viewport_height = content_area.height.to_usize();
    let content_height: usize = {
        let _guard = crate::profile!("panel::scroll_calc");
        text.iter().map(|line| count_wrapped_lines(line, viewport_width)).sum()
    };
    let max_scroll = content_height.saturating_sub(viewport_height).to_f32();
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = {
        let _guard = crate::profile!("panel::paragraph_new");
        Paragraph::new(text)
            .style(base_style)
            .wrap(Wrap { trim: false })
            .scroll((state.scroll_offset.round().to_u16(), 0))
    };

    {
        let _guard = crate::profile!("panel::frame_render");
        frame.render_widget(paragraph, content_area);
    }
}
