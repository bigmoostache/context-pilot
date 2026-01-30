use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::actions::Action;
use crate::constants::{SIDEBAR_WIDTH, CONTEXT_LIST_START_ROW};
use crate::state::State;

/// Handle mouse events and return appropriate action
pub fn handle_mouse(event: &MouseEvent, state: &State) -> Action {
    let x = event.column;
    let y = event.row;

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => handle_left_click(x, y, state),
        _ => Action::None,
    }
}

/// Handle left mouse button click
fn handle_left_click(x: u16, y: u16, state: &State) -> Action {
    // Check if click is in sidebar
    if x < SIDEBAR_WIDTH {
        return handle_sidebar_click(x, y, state);
    }

    // Click in main content area - could add message selection later
    Action::None
}

/// Handle clicks in the sidebar area
fn handle_sidebar_click(_x: u16, y: u16, state: &State) -> Action {
    let context_count = state.context.len();

    // Check if click is on a context item
    // Context items are at rows CONTEXT_LIST_START_ROW to CONTEXT_LIST_START_ROW + context_count - 1
    if y >= CONTEXT_LIST_START_ROW && y < CONTEXT_LIST_START_ROW + context_count as u16 {
        let clicked_index = (y - CONTEXT_LIST_START_ROW) as usize;
        if clicked_index < context_count {
            return Action::SelectContext(clicked_index);
        }
    }

    Action::None
}
