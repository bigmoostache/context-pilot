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

    // Count fixed contexts at the start to find separator position
    let fixed_count = state.context.iter()
        .take_while(|c| c.context_type.is_fixed())
        .count();

    // There's a separator line if we have both fixed and non-fixed contexts
    let has_separator = fixed_count > 0 && fixed_count < context_count;
    let separator_row = if has_separator {
        CONTEXT_LIST_START_ROW + fixed_count as u16
    } else {
        u16::MAX // No separator
    };

    // Total rows including separator
    let total_rows = context_count as u16 + if has_separator { 1 } else { 0 };

    if y >= CONTEXT_LIST_START_ROW && y < CONTEXT_LIST_START_ROW + total_rows {
        let visual_row = y - CONTEXT_LIST_START_ROW;

        // Ignore clicks on the separator line
        if has_separator && y == separator_row {
            return Action::None;
        }

        // Calculate actual context index, accounting for separator
        let clicked_index = if has_separator && y > separator_row {
            visual_row as usize - 1 // Subtract 1 for the separator line
        } else {
            visual_row as usize
        };

        if clicked_index < context_count {
            return Action::SelectContext(clicked_index);
        }
    }

    Action::None
}
