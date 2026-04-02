//! Conversation IR adapter — renders the conversation region to ratatui.
//!
//! Wraps the existing `ConversationPanel` content builder (with its
//! multi-level caching) in IR-controlled chrome: border, title, scrollbar,
//! and auto-scroll logic. The heavy message rendering still lives in
//! `modules::conversation::render` — full IR migration is Phase 6+.

use ratatui::prelude::{Frame, Line, Margin, Rect, Span, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::state::State;
use crate::ui::theme;
use cp_base::cast::Safe as _;

/// Render the conversation panel with IR-controlled chrome and scrollbar.
///
/// The content (messages, streaming tools, input) comes from the existing
/// `ConversationPanel::build_content_cached()`, preserving the multi-level
/// cache. The chrome (border, title, scrollbar, auto-scroll) is driven by
/// the IR snapshot.
pub(crate) fn render_conversation_from_ir(
    frame: &mut Frame<'_>,
    state: &mut State,
    area: Rect,
    conversation: &cp_render::conversation::Conversation,
) {
    let base_style = Style::default().bg(theme::bg_surface());

    // Title reflects streaming state
    let title = if !conversation.streaming_tools.is_empty() || state.flags.stream.phase.is_streaming() {
        "Conversation *"
    } else {
        "Conversation"
    };

    let inner_area = Rect::new(area.x.saturating_add(1), area.y, area.width.saturating_sub(2), area.height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::border()))
        .style(base_style)
        .title(Span::styled(format!(" {title} "), Style::default().fg(theme::accent()).bold()));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    // Update viewport width BEFORE building content so it can pre-wrap lines
    state.last_viewport_width = content_area.width;

    // Use the existing cached content builder (multi-level: full → per-message → input)
    let text = build_content_cached(state, base_style);

    // Each Line = 1 visual line (pre-wrapped in render_message)
    let viewport_height = content_area.height.to_usize();
    let content_height = text.len();

    let max_scroll = content_height.saturating_sub(viewport_height).to_f32();
    state.max_scroll = max_scroll;

    // Auto-scroll: snap to bottom unless user manually scrolled up
    if state.flags.stream.user_scrolled && state.scroll_offset >= max_scroll - 0.5 {
        state.flags.stream.user_scrolled = false;
    }
    if !state.flags.stream.user_scrolled {
        state.scroll_offset = max_scroll;
    }
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = {
        let _guard = crate::profile!("conv::paragraph_new");
        Paragraph::new(text)
            .style(base_style)
            // No .wrap() — content is pre-wrapped for performance
            .scroll((state.scroll_offset.round().to_u16(), 0))
    };

    {
        let _guard = crate::profile!("conv::frame_render");
        frame.render_widget(paragraph, content_area);
    }

    // Scrollbar (only when content overflows)
    if content_height > viewport_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(theme::bg_elevated()))
            .thumb_style(Style::default().fg(theme::accent_dim()));

        let mut scrollbar_state =
            ScrollbarState::new(max_scroll.to_usize()).position(state.scroll_offset.round().to_usize());

        frame.render_stateful_widget(
            scrollbar,
            inner_area.inner(Margin { horizontal: 0, vertical: 1 }),
            &mut scrollbar_state,
        );
    }
}

// ── Overlays ─────────────────────────────────────────────────────────

/// Render the question form overlay if the IR snapshot contains one.
///
/// Returns the height consumed by the question form (for layout splitting),
/// or `None` if no question form is active.
pub(crate) fn render_question_form_if_active(
    state: &State,
    overlays: &[cp_render::conversation::Overlay],
) -> Option<u16> {
    if !overlays.iter().any(|o| matches!(o, cp_render::conversation::Overlay::QuestionForm(_))) {
        return None;
    }

    // Delegate height calculation to existing input.rs code
    let form = state.get_ext::<cp_base::ui::question_form::PendingForm>()?;
    if form.resolved {
        return None;
    }
    let height = super::super::input::calculate_question_form_height(form);
    Some(height)
}

/// Render the autocomplete popup overlay if the IR snapshot contains one.
pub(crate) fn render_autocomplete_if_active(
    frame: &mut Frame<'_>,
    state: &State,
    content_area: Rect,
    overlays: &[cp_render::conversation::Overlay],
) {
    if !overlays.iter().any(|o| matches!(o, cp_render::conversation::Overlay::Autocomplete(_))) {
        return;
    }
    // Delegate to existing autocomplete renderer
    super::super::input::render_autocomplete_popup(frame, state, content_area);
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Delegate to the conversation panel's cached content builder.
///
/// This calls the existing conversation panel's content builder which has
/// the full multi-level caching (full hash → per-message → input).
// Here be dragons (and three layers of cache invalidation)
fn build_content_cached(state: &mut State, _base_style: Style) -> Vec<Line<'static>> {
    let blocks = crate::modules::conversation::build_content_cached(state);
    crate::ui::ir::blocks_to_lines(&blocks)
}
