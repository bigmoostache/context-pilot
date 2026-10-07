/// Character constants re-exported from the infra layer.
pub(crate) use crate::infra::constants::chars;
/// Help subsystem: config overlay, command palette, input overlays.
pub(crate) mod help;
/// Shared UI helper functions: truncation, formatting, syntax highlighting.
pub(crate) mod helpers;
/// IR-to-ratatui adapter: converts semantic blocks to terminal widgets.
pub(crate) mod ir;
/// Markdown parsing and table rendering utilities.
pub(crate) mod markdown;
/// Performance monitoring overlay and metrics.
pub(crate) mod perf;
/// Meilisearch indexing status overlay (Ctrl+I).
pub(crate) mod search_overlay;
/// Threads view: dedicated layout for thread management.
mod threads_view;
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

/// Whether the threads-list surface should be painted this frame: the Threads
/// view mode is active **and** the human has not drilled into a specific thread.
///
/// When drilled (G3), the renderer paints the drilled thread's full panel body
/// (sidebar + panels) instead of the list — `render_frame` has already swapped
/// that thread's runtime into `state`, so the normal body renders it
/// pixel-identically to its own main view.
fn showing_threads_list(state: &State) -> bool {
    state.view_mode == cp_base::state::data::config::ViewMode::Threads
        && cp_mod_threads::types::FocusState::get(state).drilled_thread_id.is_none()
}

/// Top-level render entry point: draws the entire TUI frame.
pub(crate) fn render(frame: &mut Frame<'_>, state: &mut State) {
    PERF.frame_start();
    let _guard = crate::profile!("ui::render");
    let _fg = cp_base::flame!("render");
    let area = frame.area();

    // Build the IR frame snapshot (Phase 4 integration point).
    // Phase 5 progressively replaces direct-render code paths below.
    let ir_frame = {
        let _build = crate::profile!("ir_build_frame");
        ir::build_frame(state)
    };

    // Fill base background
    {
        let _g = crate::profile!("bg_fill");
        frame.render_widget(Block::default().style(Style::default().bg(theme::bg_base())), area);
    }

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
    {
        let _status = crate::profile!("status_bar");
        ir::render_status_bar::render_status_bar_from_ir(frame, &ir_frame.status_bar, status_area);
    }

    // Render autocomplete popup if active (via IR overlays).
    // In Threads mode the input lives inside the right pane (past the thread
    // list), so offset by THREAD_LIST_WIDTH instead of the sidebar width.
    {
        let _g = crate::profile!("autocomplete");
        let offset =
            if showing_threads_list(state) { threads_view::THREAD_LIST_WIDTH } else { state.view_mode.width() };
        let content_x = area.x.saturating_add(offset);
        let content_width = area.width.saturating_sub(offset);
        let content_height = area.height.saturating_sub(STATUS_BAR_HEIGHT);
        let content_area = Rect::new(content_x, area.y, content_width, content_height);
        ir::render_conversation::render_autocomplete_if_active(frame, content_area, &ir_frame.overlays);
    }

    {
        let _overlays = crate::profile!("modal_overlays");
        render_modal_overlays(frame, area, &ir_frame.overlays);
    }

    PERF.frame_end();
}

/// Render the full-area modal overlays (perf monitor, config, search-index) from
/// the IR overlay stack. The autocomplete popup is handled separately (it needs
/// the content-area offset), so it is not touched here.
fn render_modal_overlays(frame: &mut Frame<'_>, area: Rect, overlays: &[cp_render::conversation::Overlay]) {
    // Render performance overlay if active (from IR overlays)
    if let Some(perf_overlay) = overlays.iter().find_map(|o| {
        cp_macros::deref_match!(o, {
            cp_render::conversation::Overlay::Perf(ref p) => Some(p),
            cp_render::conversation::Overlay::QuestionForm(_)
            | cp_render::conversation::Overlay::Autocomplete(_)
            | cp_render::conversation::Overlay::Config(_)
            | cp_render::conversation::Overlay::CommandPalette(_)
            | cp_render::conversation::Overlay::SearchIndex(_) => None,
        })
    }) {
        perf::render_perf_overlay_from_ir(frame, area, perf_overlay);
    }

    // Render config overlay if active (from IR overlays)
    if let Some(config_overlay) = overlays.iter().find_map(|o| {
        cp_macros::deref_match!(o, {
            cp_render::conversation::Overlay::Config(ref c) => Some(c),
            cp_render::conversation::Overlay::QuestionForm(_)
            | cp_render::conversation::Overlay::Autocomplete(_)
            | cp_render::conversation::Overlay::Perf(_)
            | cp_render::conversation::Overlay::CommandPalette(_)
            | cp_render::conversation::Overlay::SearchIndex(_) => None,
        })
    }) {
        help::config_overlay::render_config_overlay(frame, config_overlay, area);
    }

    // Render Meilisearch indexing status overlay if active (from IR overlays)
    if let Some(search_overlay) = overlays.iter().find_map(|o| {
        cp_macros::deref_match!(o, {
            cp_render::conversation::Overlay::SearchIndex(ref s) => Some(s.as_ref()),
            cp_render::conversation::Overlay::QuestionForm(_)
            | cp_render::conversation::Overlay::Autocomplete(_)
            | cp_render::conversation::Overlay::Perf(_)
            | cp_render::conversation::Overlay::Config(_)
            | cp_render::conversation::Overlay::CommandPalette(_) => None,
        })
    }) {
        search_overlay::render_search_index_overlay(frame, search_overlay, area);
    }
}

/// Render the body area: sidebar (if visible) and main content panel,
/// or the threads view when `ViewMode::Threads` is active.
fn render_body(frame: &mut Frame<'_>, state: &mut State, area: Rect, ir_frame: &cp_render::frame::Frame) {
    // Threads mode: completely different layout (no sidebar, no panels) —
    // unless the human has drilled into a thread (G3), in which case fall
    // through to the normal body to paint that thread's full panel view.
    if showing_threads_list(state) {
        let _threads = crate::profile!("threads_view");
        threads_view::render_threads_view(frame, state, area);
        return;
    }

    let sw = state.view_mode.width();

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
    {
        let _g = crate::profile!("sidebar_draw");
        ir::render_sidebar::render_sidebar_from_ir(frame, &ir_frame.sidebar, sidebar_area);
    }
    render_main_content(frame, state, content_area, ir_frame);
}

/// Render the main content area.
fn render_main_content(frame: &mut Frame<'_>, state: &mut State, area: Rect, ir_frame: &cp_render::frame::Frame) {
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
        let _g = crate::profile!("conversation_draw");
        ir::render_conversation::render_conversation_from_ir(frame, state, area, &ir_frame.conversation);
    } else {
        let _g = crate::infra::profiler::dyn_guard("panel_draw_", context_type.as_str());
        ir::render_panel::render_panel_from_ir(frame, state, area, &ir_frame.active_panel);
    }
}
