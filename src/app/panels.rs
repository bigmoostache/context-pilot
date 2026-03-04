//! Panel trait and implementations for different context types.
//!
//! The `Panel` trait and core types live in `cp_base::panels`.
//! This module re-exports them and adds binary-specific functionality
//! (rendering with theme/profiling, panel registry).

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::{ContextType, State};
use crate::ui::{helpers::count_wrapped_lines, theme};
use cp_base::cast::SafeCast;

// Re-export the Panel trait, ContextItem, and utility functions from cp-base
pub(crate) use cp_base::panels::{ContextItem, Panel, now_ms, paginate_content, update_if_changed};

/// Render a panel with the binary's full chrome (borders, theme, scroll, profiling).
/// This is NOT part of the Panel trait — it uses binary-specific deps (theme, profile!, UI helpers).
pub(crate) fn render_panel_default(panel: &dyn Panel, frame: &mut Frame<'_>, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::bg_surface());
    let title = panel.title(state);

    let inner_area = Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), area.height);

    // Build bottom title for dynamic panels: "refreshed Xs ago"
    let bottom_title =
        state.context.get(state.selected_context).filter(|ctx| !ctx.context_type.is_fixed()).and_then(|ctx| {
            let ts = ctx.last_refresh_ms;
            if ts < 1_577_836_800_000 {
                return None;
            } // invalid timestamp
            let now = now_ms();
            if now <= ts {
                return None;
            }
            Some(format!(" {} ", crate::ui::helpers::format_time_ago(now - ts)))
        });

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::border()))
        .style(base_style)
        .title(Span::styled(format!(" {title} "), Style::default().fg(theme::accent()).bold()));

    if let Some(ref bottom) = bottom_title {
        block = block.title_bottom(Span::styled(bottom, Style::default().fg(theme::text_muted())));
    }

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let text = panel.content(state, base_style);

    // Calculate and set max scroll (accounting for wrapped lines)
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

/// Get the appropriate panel for a context type (delegates to module system).
/// Returns a no-op fallback for orphaned context types (e.g., removed modules).
pub(crate) fn get_panel(context_type: &ContextType) -> Box<dyn Panel> {
    crate::modules::create_panel(context_type).unwrap_or_else(|| Box::new(FallbackPanel))
}

/// Minimal panel for context types whose module has been removed.
struct FallbackPanel;

impl Panel for FallbackPanel {
    fn title(&self, _state: &State) -> String {
        "(removed)".to_string()
    }
    fn content(&self, _state: &State, _base_style: Style) -> Vec<Line<'static>> {
        vec![Line::from("Panel module no longer available")]
    }
}

/// Refresh all panels (update token counts, etc.)
pub(crate) fn refresh_all_panels(state: &mut State) {
    // Get unique context types from state
    let context_types: Vec<ContextType> = state.context.iter().map(|c| c.context_type.clone()).collect();

    for context_type in &context_types {
        let panel = get_panel(context_type);
        panel.refresh(state);
    }
}

/// Collect all context items from all panels
pub(crate) fn collect_all_context(state: &State) -> Vec<ContextItem> {
    let mut items = Vec::new();

    // Get UNIQUE context types from state (dedup to avoid multiplying items!)
    let mut seen = std::collections::HashSet::new();
    let context_types: Vec<ContextType> =
        state.context.iter().map(|c| c.context_type.clone()).filter(|ct| seen.insert(ct.clone())).collect();

    for context_type in &context_types {
        let panel = get_panel(context_type);
        items.extend(panel.context(state));
    }

    items
}
