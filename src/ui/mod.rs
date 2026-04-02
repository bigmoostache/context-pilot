/// Character constants re-exported from the infra layer.
pub(crate) use crate::infra::constants::chars;
/// Help subsystem: config overlay and command palette.
pub(crate) mod help;
/// Shared UI helper functions: truncation, formatting, syntax highlighting.
pub(crate) mod helpers;
/// Status bar, question form, and autocomplete popup rendering.
mod input;
/// IR-to-ratatui adapter: converts semantic blocks to terminal widgets.
pub(crate) mod ir;
/// Markdown parsing and table rendering utilities.
pub(crate) mod markdown;
/// Performance monitoring overlay and metrics.
pub(crate) mod perf;
/// Theme color constants re-exported from the infra layer.
pub(crate) use crate::infra::constants::theme;
/// Typewriter animation buffer re-exported from helpers.
pub(crate) use helpers::TypewriterBuffer;

use ratatui::Frame;
use ratatui::prelude::{Constraint, Direction, Layout, Rect, Style};
use ratatui::widgets::Block;

use crate::infra::constants::STATUS_BAR_HEIGHT;
use crate::state::{Kind, State};
use crate::ui::perf::PERF;

/// Top-level render entry point: draws the entire TUI frame.
pub(crate) fn render(frame: &mut Frame<'_>, state: &mut State) {
    PERF.frame_start();
    let _guard = crate::profile!("ui::render");
    let area = frame.area();

    // Build the IR frame snapshot (Phase 4 integration point).
    // Phase 5 progressively replaces direct-render code paths below.
    let ir_frame = ir::build_frame(state);

    // Fill base background
    frame.render_widget(Block::default().style(Style::default().bg(theme::bg_base())), area);

    // Main layout: body + footer (no header)
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                    // Body
            Constraint::Length(STATUS_BAR_HEIGHT), // Status bar
        ])
        .split(area);

    let (Some(&body_area), Some(&status_area)) = (main_layout.first(), main_layout.get(1)) else {
        debug_assert!(false, "main_layout must have at least 2 chunks");
        return;
    };
    render_body(frame, state, body_area, &ir_frame);
    ir::render_status_bar::render_status_bar_from_ir(frame, &ir_frame.status_bar, status_area, state.spinner_frame);

    // Render performance overlay if enabled
    if state.flags.ui.perf_enabled {
        perf::render_perf_overlay(frame, area);
    }

    // Render autocomplete popup if active (via IR overlays)
    {
        let sw = state.sidebar_mode.width();
        let content_x = area.x.saturating_add(sw);
        let content_width = area.width.saturating_sub(sw);
        let content_height = area.height.saturating_sub(STATUS_BAR_HEIGHT);
        let content_area = Rect::new(content_x, area.y, content_width, content_height);
        ir::render_conversation::render_autocomplete_if_active(frame, state, content_area, &ir_frame.overlays);
    }

    // Render config overlay if open
    if state.flags.config.config_view {
        help::config_overlay::render_config_overlay(frame, state, area);
    }

    PERF.frame_end();
}

/// Render the body area: sidebar (if visible) and main content panel.
fn render_body(frame: &mut Frame<'_>, state: &mut State, area: Rect, ir_frame: &cp_render::frame::Frame) {
    let sw = state.sidebar_mode.width();
    if sw == 0 {
        // Hidden mode — no sidebar at all
        render_main_content(frame, state, area, ir_frame);
        return;
    }

    // Body layout: sidebar + main content
    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(sw), // Sidebar
            Constraint::Min(1),     // Main content
        ])
        .split(area);

    let (Some(&sidebar_area), Some(&content_area)) = (body_layout.first(), body_layout.get(1)) else {
        debug_assert!(false, "body_layout must have at least 2 chunks");
        return;
    };
    ir::render_sidebar::render_sidebar_from_ir(frame, &ir_frame.sidebar, sidebar_area);
    render_main_content(frame, state, content_area, ir_frame);
}

/// Render the main content area, splitting for question form if active.
fn render_main_content(frame: &mut Frame<'_>, state: &mut State, area: Rect, ir_frame: &cp_render::frame::Frame) {
    // Check if question form is active via IR overlays
    if let Some(form_height) = ir::render_conversation::render_question_form_if_active(state, &ir_frame.overlays) {
        // Split: content panel on top, question form at bottom
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),              // Content panel (shrinks)
                Constraint::Length(form_height), // Question form
            ])
            .split(area);

        let (Some(&panel_area), Some(&raw_form_area)) = (layout.first(), layout.get(1)) else {
            debug_assert!(false, "question form layout must have at least 2 chunks");
            return;
        };
        render_content_panel(frame, state, panel_area, ir_frame);
        // Indent form by 1 col to avoid overlapping sidebar border
        let form_area = Rect {
            x: raw_form_area.x.saturating_add(1),
            width: raw_form_area.width.saturating_sub(1),
            ..raw_form_area
        };
        input::render_question_form(frame, state, form_area);
        return;
    }

    // Normal rendering — no separate input box, panels handle their own
    render_content_panel(frame, state, area, ir_frame);
}

/// Render the active content panel (conversation or generic panel).
fn render_content_panel(frame: &mut Frame<'_>, state: &mut State, area: Rect, ir_frame: &cp_render::frame::Frame) {
    let _guard = crate::profile!("ui::render_panel");
    let context_type = state
        .context
        .get(state.selected_context)
        .map_or_else(|| Kind::new(Kind::CONVERSATION), |c| c.context_type.clone());

    // ConversationPanel renders from its multi-level cached content builder,
    // wrapped in IR-controlled chrome (border, scrollbar, auto-scroll).
    // All other panels render from the IR snapshot, falling back to content()
    // for panels whose blocks() returns empty (not yet migrated).
    if context_type.as_str() == Kind::CONVERSATION {
        ir::render_conversation::render_conversation_from_ir(frame, state, area, &ir_frame.conversation);
    } else {
        ir::render_panel::render_panel_from_ir(frame, state, area, &ir_frame.active_panel);
    }
}
