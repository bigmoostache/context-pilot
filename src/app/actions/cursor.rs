//! Cursor movement, text editing helpers, and command expansion logic.

use super::helpers::eject_cursor_from_sentinel;
use crate::state::State;
use cp_mod_prompt::types::PromptState;

/// Handle `/command` expansion after typing space or newline.
pub(super) fn handle_command_expansion(state: &mut State) {
    // Find start of current "word" — scan back past the space we just inserted
    let before_space = state.input_cursor.saturating_sub(1); // position of the space
    let bytes = state.input.as_bytes();
    let mut word_start = before_space;
    // Scan backwards to find word boundary (newline, space, or sentinel \x00)
    while word_start > 0 {
        let Some(&prev_byte) = bytes.get(word_start.saturating_sub(1)) else { break };
        if prev_byte == b'\n' || prev_byte == b' ' || prev_byte == 0 {
            break;
        }
        word_start = word_start.saturating_sub(1);
    }
    // Ensure we land on a valid char boundary (backward scan is byte-level)
    while word_start < before_space && !state.input.is_char_boundary(word_start) {
        word_start = word_start.saturating_add(1);
    }
    let word = state.input.get(word_start..before_space).unwrap_or("");
    if let Some(cmd_name) = word.strip_prefix('/') {
        let cmd_content =
            PromptState::get(state).commands.iter().find(|cmd| cmd.id == cmd_name).map(|cmd| cmd.content.clone());
        if let Some(content) = cmd_content {
            let label = cmd_name.to_string();
            let idx = state.paste_buffers.len();
            state.paste_buffers.push(content);
            state.paste_buffer_labels.push(Some(label));
            let sentinel = format!("\x00{idx}\x00");
            // Replace /command<space> with sentinel
            state.input = format!(
                "{}{}\n{}",
                state.input.get(..word_start).unwrap_or(""),
                sentinel,
                state.input.get(state.input_cursor..).unwrap_or(""),
            );
            state.input_cursor = word_start.saturating_add(sentinel.len()).saturating_add(1);
        }
    }
}

/// Handle backspace, including paste sentinel removal.
pub(super) fn handle_input_backspace(state: &mut State) {
    if state.input_cursor == 0 {
        return;
    }
    let bytes = state.input.as_bytes();
    let cursor_prev = state.input_cursor.saturating_sub(1);

    // Check if we're at the end of a paste sentinel (\x00{idx}\x00)
    // The closing \x00 is at cursor-1
    let Some(&prev_b) = bytes.get(cursor_prev) else { return };

    if prev_b == 0 {
        // Find the opening \x00 by scanning backwards past the index digits
        let mut scan = state.input_cursor.saturating_sub(2); // skip closing \x00
        while let Some(&b) = bytes.get(scan) {
            if b == 0 || scan == 0 {
                break;
            }
            scan = scan.saturating_sub(1);
        }
        let Some(&scan_b) = bytes.get(scan) else { return };
        if scan_b == 0 {
            // Remove the entire sentinel from scan..cursor
            state.input = format!(
                "{}{}",
                state.input.get(..scan).unwrap_or(""),
                state.input.get(state.input_cursor..).unwrap_or("")
            );
            state.input_cursor = scan;
        }
    } else if state.input_cursor >= 2 && prev_b.is_ascii_digit() {
        // Check if cursor is inside a sentinel (between \x00 and closing \x00)
        // Scan backwards to see if we hit \x00 before any non-digit
        let mut scan = cursor_prev;
        while let Some(&b) = bytes.get(scan) {
            if !b.is_ascii_digit() || scan == 0 {
                break;
            }
            scan = scan.saturating_sub(1);
        }
        let Some(&scan_b) = bytes.get(scan) else { return };
        if scan_b == 0 {
            // We're inside a sentinel — find the closing \x00
            let mut end = state.input_cursor;
            while let Some(&b) = bytes.get(end) {
                if b == 0 {
                    break;
                }
                end = end.saturating_add(1);
            }
            if let Some(&b) = bytes.get(end)
                && b == 0
            {
                end = end.saturating_add(1); // include closing \x00
            }
            state.input = format!("{}{}", state.input.get(..scan).unwrap_or(""), state.input.get(end..).unwrap_or(""));
            state.input_cursor = scan;
        } else {
            // Not a sentinel — normal backspace
            normal_backspace(state);
        }
    } else {
        // Normal backspace — remove one character
        normal_backspace(state);
    }
}

/// Remove one character before the cursor (normal backspace).
fn normal_backspace(state: &mut State) {
    let prev = state.input.get(..state.input_cursor).unwrap_or("").char_indices().last().map_or(0, |(i, _)| i);
    let _r = state.input.remove(prev);
    state.input_cursor = prev;
}

/// Handle `CursorWordLeft` — move cursor to the start of the previous word.
pub(super) fn handle_cursor_word_left(state: &mut State) {
    if state.input_cursor > 0 {
        let before = state.input.get(..state.input_cursor).unwrap_or("");
        let trimmed = before.trim_end();
        if trimmed.is_empty() {
            state.input_cursor = 0;
        } else {
            let word_start = trimmed.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i.saturating_add(1));
            state.input_cursor = word_start;
        }
        state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
    }
}

/// Handle `CursorWordRight` — move cursor to the start of the next word.
pub(super) fn handle_cursor_word_right(state: &mut State) {
    if state.input_cursor < state.input.len() {
        let after = state.input.get(state.input_cursor..).unwrap_or("");
        let skip_word = after.find(|c: char| c.is_whitespace()).unwrap_or(after.len());
        let remaining = after.get(skip_word..).unwrap_or("");
        let skip_space = remaining.find(|c: char| !c.is_whitespace()).unwrap_or(remaining.len());
        state.input_cursor = state.input_cursor.saturating_add(skip_word.saturating_add(skip_space));
        state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
    }
}

/// Handle `DeleteWordLeft` — delete the word before the cursor.
pub(super) fn handle_delete_word_left(state: &mut State) {
    if state.input_cursor > 0 {
        let before = state.input.get(..state.input_cursor).unwrap_or("");
        let trimmed = before.trim_end();
        let word_start = if trimmed.is_empty() {
            0
        } else {
            trimmed.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i.saturating_add(1))
        };
        state.input = format!(
            "{}{}",
            state.input.get(..word_start).unwrap_or(""),
            state.input.get(state.input_cursor..).unwrap_or("")
        );
        state.input_cursor = word_start;
    }
}

/// Handle `RemoveListItem` — delete from line start to cursor.
pub(super) fn handle_remove_list_item(state: &mut State) {
    if state.input_cursor > 0 {
        let before = state.input.get(..state.input_cursor).unwrap_or("");
        let line_start = before.rfind('\n').map_or(0, |i| i.saturating_add(1));
        state.input = format!(
            "{}{}",
            state.input.get(..line_start).unwrap_or(""),
            state.input.get(state.input_cursor..).unwrap_or("")
        );
        state.input_cursor = line_start;
    }
}

/// Handle `CursorHome` — move cursor to beginning of current line.
pub(super) fn handle_cursor_home(state: &mut State) {
    let before_cursor = state.input.get(..state.input_cursor).unwrap_or("");
    state.input_cursor = before_cursor.rfind('\n').map_or(0, |i| i.saturating_add(1));
    state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
}

/// Handle `CursorEnd` — move cursor to end of current line.
pub(super) fn handle_cursor_end(state: &mut State) {
    let after_cursor = state.input.get(state.input_cursor..).unwrap_or("");
    state.input_cursor = state.input_cursor.saturating_add(after_cursor.find('\n').unwrap_or(after_cursor.len()));
    state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
}
