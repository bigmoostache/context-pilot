pub mod chars;
pub mod helpers;
mod input;
pub mod markdown;
mod sidebar;
pub mod spinner;
pub mod theme;

use ratatui::{
    prelude::*,
    widgets::Block,
};

use crate::constants::{SIDEBAR_WIDTH, STATUS_BAR_HEIGHT};
use crate::panels;
use crate::state::{ContextType, State};


pub fn render(frame: &mut Frame, state: &mut State) {
    let _guard = crate::profile!("ui::render");
    let area = frame.area();

    // Fill base background
    frame.render_widget(
        Block::default().style(Style::default().bg(theme::BG_BASE)),
        area
    );

    // Main layout: body + footer (no header)
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                        // Body
            Constraint::Length(STATUS_BAR_HEIGHT),    // Status bar
        ])
        .split(area);

    render_body(frame, state, main_layout[0]);
    input::render_status_bar(frame, state, main_layout[1]);
}

fn render_body(frame: &mut Frame, state: &mut State, area: Rect) {
    // Body layout: sidebar + main content
    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(SIDEBAR_WIDTH),  // Sidebar
            Constraint::Min(1),                 // Main content
        ])
        .split(area);

    sidebar::render_sidebar(frame, state, body_layout[0]);
    render_main_content(frame, state, body_layout[1]);
}

fn render_main_content(frame: &mut Frame, state: &mut State, area: Rect) {
    // No separate input box - panels handle their own input display
    render_content_panel(frame, state, area);
}

fn render_content_panel(frame: &mut Frame, state: &mut State, area: Rect) {
    let _guard = crate::profile!("ui::render_panel");
    let context_type = state.context.get(state.selected_context)
        .map(|c| c.context_type)
        .unwrap_or(ContextType::Conversation);

    let panel = panels::get_panel(context_type);
    panel.render(frame, state, area);
}
