mod chars;
mod conversation;
mod helpers;
mod input;
mod markdown;
mod panels;
mod sidebar;
mod theme;

use ratatui::{
    prelude::*,
    widgets::Block,
};

use crate::constants::{SIDEBAR_WIDTH, STATUS_BAR_HEIGHT, INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT};
use crate::state::{ContextType, State};


pub fn render(frame: &mut Frame, state: &mut State) {
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
    // Calculate input height based on content
    let input_lines = state.input.lines().count().max(1);
    let input_height = (input_lines as u16 + 2).clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT);

    let content_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                    // Content panel
            Constraint::Length(input_height),      // Input area
        ])
        .split(area);

    render_content_panel(frame, state, content_layout[0]);
    input::render_input(frame, state, content_layout[1]);
}

fn render_content_panel(frame: &mut Frame, state: &mut State, area: Rect) {
    let selected = state.context.get(state.selected_context);

    match selected.map(|c| c.context_type) {
        Some(ContextType::Conversation) => conversation::render_conversation(frame, state, area),
        Some(ContextType::File) => panels::render_file(frame, state, area),
        Some(ContextType::Tree) => panels::render_tree(frame, state, area),
        Some(ContextType::Glob) => panels::render_glob(frame, state, area),
        Some(ContextType::Tmux) => panels::render_tmux(frame, state, area),
        Some(ContextType::Todo) => panels::render_todo(frame, state, area),
        Some(ContextType::Memory) => panels::render_memory(frame, state, area),
        Some(ContextType::Overview) => panels::render_overview(frame, state, area),
        Some(ContextType::Tools) => panels::render_tools(frame, state, area),
        None => conversation::render_conversation(frame, state, area),
    }
}
