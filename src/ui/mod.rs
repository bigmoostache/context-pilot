pub(crate) use crate::infra::constants::chars;
pub(crate) mod help;
pub(crate) mod helpers;
mod input;
pub(crate) mod markdown;
pub(crate) mod perf;
mod sidebar;
pub(crate) use crate::infra::constants::theme;
pub(crate) mod typewriter;

use ratatui::{prelude::*, widgets::Block};

use crate::app::panels;
use crate::infra::constants::STATUS_BAR_HEIGHT;
use crate::state::{ContextType, State};
use crate::ui::perf::PERF;

pub(crate) fn render(frame: &mut Frame<'_>, state: &mut State) {
    PERF.frame_start();
    let _guard = crate::profile!("ui::render");
    let area = frame.area();

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

    render_body(frame, state, main_layout[0]);
    input::render_status_bar(frame, state, main_layout[1]);

    // Render performance overlay if enabled
    if state.flags.perf_enabled {
        perf::render_perf_overlay(frame, area);
    }

    // Render autocomplete popup if active
    if let Some(ac) = state.get_ext::<cp_base::autocomplete::AutocompleteState>()
        && ac.active
    {
        // Position in main content area (right of sidebar, above status bar)
        let sw = state.sidebar_mode.width();
        let content_x = area.x + sw;
        let content_width = area.width.saturating_sub(sw);
        let content_height = area.height.saturating_sub(STATUS_BAR_HEIGHT);
        let content_area = Rect::new(content_x, area.y, content_width, content_height);
        input::render_autocomplete_popup(frame, state, content_area);
    }

    // Render config overlay if open
    if state.flags.config_view {
        help::config_overlay::render_config_overlay(frame, state, area);
    }

    PERF.frame_end();
}

fn render_body(frame: &mut Frame<'_>, state: &mut State, area: Rect) {
    let sw = state.sidebar_mode.width();
    if sw == 0 {
        // Hidden mode — no sidebar at all
        render_main_content(frame, state, area);
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

    match state.sidebar_mode {
        cp_base::state::SidebarMode::Normal => {
            sidebar::render_sidebar(frame, state, body_layout[0]);
        }
        cp_base::state::SidebarMode::Collapsed => {
            sidebar::render_sidebar_collapsed(frame, state, body_layout[0]);
        }
        cp_base::state::SidebarMode::Hidden => {} // handled above
    }
    render_main_content(frame, state, body_layout[1]);
}

fn render_main_content(frame: &mut Frame<'_>, state: &mut State, area: Rect) {
    // Check if question form is active — render it at bottom of content area
    if let Some(form) = state.get_ext::<cp_base::ui::PendingQuestionForm>()
        && !form.resolved
    {
        // Split: content panel on top, question form at bottom
        let form_height = input::calculate_question_form_height(form);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),              // Content panel (shrinks)
                Constraint::Length(form_height), // Question form
            ])
            .split(area);

        render_content_panel(frame, state, layout[0]);
        // Indent form by 1 col to avoid overlapping sidebar border
        let form_area = Rect { x: layout[1].x + 1, width: layout[1].width.saturating_sub(1), ..layout[1] };
        input::render_question_form(frame, state, form_area);
        return;
    }

    // Normal rendering — no separate input box, panels handle their own
    render_content_panel(frame, state, area);
}

fn render_content_panel(frame: &mut Frame<'_>, state: &mut State, area: Rect) {
    let _guard = crate::profile!("ui::render_panel");
    let context_type = state
        .context
        .get(state.selected_context)
        .map_or(ContextType::new(ContextType::CONVERSATION), |c| c.context_type.clone());

    let panel = panels::get_panel(&context_type);

    // ConversationPanel overrides render() with custom scrollbar + caching.
    // All other panels use render_panel_default (which calls panel.content()).
    if context_type == ContextType::CONVERSATION {
        panel.render(frame, state, area);
    } else {
        panels::render_panel_default(panel.as_ref(), frame, state, area);
    }
}
