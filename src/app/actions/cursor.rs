//! Cursor movement, text editing, selection management, and command expansion logic.

use super::helpers::eject_cursor_from_sentinel;
use crate::state::State;

// ── Selection helpers ────────────────────────────────────────────────

/// Get the ordered selection range (start, end) if a selection is active.
fn selection_range(state: &State) -> Option<(usize, usize)> {
    state.input_selection_anchor.map(|anchor| (anchor.min(state.input_cursor), anchor.max(state.input_cursor)))
}

/// Delete selected text and collapse cursor to selection start.
/// Returns `true` if there was a non-empty selection that was deleted.
pub(super) fn delete_selection(state: &mut State) -> bool {
    let Some((start, end)) = selection_range(state) else { return false };
    if start == end {
        state.input_selection_anchor = None;
        return false;
    }
    state.input = format!("{}{}", state.input.get(..start).unwrap_or(""), state.input.get(end..).unwrap_or(""));
    state.input_cursor = start;
    state.input_selection_anchor = None;
    true
}

/// Ensure selection anchor is set (for Shift+movement). If no anchor yet, set it to current cursor.
const fn extend_selection(state: &mut State) {
    if state.input_selection_anchor.is_none() {
        state.input_selection_anchor = Some(state.input_cursor);
    }
}

// ── Sentinel detection ───────────────────────────────────────────────

/// Find the paste sentinel (\x00{digits}\x00) that contains byte position `pos`, if any.
/// Returns `(start, end)` where `start` is the opening \x00 position and `end` is one past
/// the closing \x00.
fn find_enclosing_sentinel(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos >= bytes.len() {
        return None;
    }
    let &b = bytes.get(pos)?;

    if b == 0 {
        // Try as opening \x00: digits then closing \x00
        let mut end = pos.saturating_add(1);
        while end < bytes.len() && bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end = end.saturating_add(1);
        }
        if end > pos.saturating_add(1) && bytes.get(end) == Some(&0) {
            return Some((pos, end.saturating_add(1)));
        }
        // Try as closing \x00: digits preceded by opening \x00
        if pos > 0 {
            let mut start = pos;
            while start > 0 && bytes.get(start.saturating_sub(1)).is_some_and(u8::is_ascii_digit) {
                start = start.saturating_sub(1);
            }
            if start < pos && start > 0 && bytes.get(start.saturating_sub(1)) == Some(&0) {
                return Some((start.saturating_sub(1), pos.saturating_add(1)));
            }
        }
    } else if b.is_ascii_digit() {
        // Might be inside sentinel digits — scan both directions
        let mut start = pos;
        while start > 0 && bytes.get(start.saturating_sub(1)).is_some_and(u8::is_ascii_digit) {
            start = start.saturating_sub(1);
        }
        if start > 0 && bytes.get(start.saturating_sub(1)) == Some(&0) {
            let mut end = pos.saturating_add(1);
            while end < bytes.len() && bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end = end.saturating_add(1);
            }
            if bytes.get(end) == Some(&0) {
                return Some((start.saturating_sub(1), end.saturating_add(1)));
            }
        }
    }
    None
}

/// When moving left, skip backward over any sentinel at `pos`.
fn skip_sentinel_left(input: &str, pos: usize) -> usize {
    find_enclosing_sentinel(input.as_bytes(), pos).map_or(pos, |(start, _)| start)
}

/// When moving right, skip forward over any sentinel at `pos`.
fn skip_sentinel_right(input: &str, pos: usize) -> usize {
    find_enclosing_sentinel(input.as_bytes(), pos).map_or(pos, |(_, end)| end)
}

// ── Raw movement helpers (no selection management) ───────────────────

/// Compute cursor position one character to the left, skipping sentinels.
fn compute_char_left(input: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let before = input.get(..cursor).unwrap_or("");
    let new_pos = before.char_indices().last().map_or(0, |(i, _)| i);
    skip_sentinel_left(input, new_pos)
}

/// Compute cursor position one character to the right, skipping sentinels.
fn compute_char_right(input: &str, cursor: usize) -> usize {
    if cursor >= input.len() {
        return cursor;
    }
    let ch_len = input.get(cursor..).unwrap_or("").chars().next().map_or(0, char::len_utf8);
    let new_pos = cursor.saturating_add(ch_len);
    skip_sentinel_right(input, new_pos)
}

/// Move cursor to the start of the previous word.
fn move_word_left(state: &mut State) {
    if state.input_cursor > 0 {
        let before = state.input.get(..state.input_cursor).unwrap_or("");
        let trimmed = before.trim_end();
        if trimmed.is_empty() {
            state.input_cursor = 0;
        } else {
            state.input_cursor = trimmed.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i.saturating_add(1));
        }
        state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
    }
}

/// Move cursor to the start of the next word.
fn move_word_right(state: &mut State) {
    if state.input_cursor < state.input.len() {
        let after = state.input.get(state.input_cursor..).unwrap_or("");
        let skip_word = after.find(|c: char| c.is_whitespace()).unwrap_or(after.len());
        let remaining = after.get(skip_word..).unwrap_or("");
        let skip_space = remaining.find(|c: char| !c.is_whitespace()).unwrap_or(remaining.len());
        state.input_cursor = state.input_cursor.saturating_add(skip_word.saturating_add(skip_space));
        state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
    }
}

/// Move cursor to the beginning of the current line.
fn move_home(state: &mut State) {
    let before_cursor = state.input.get(..state.input_cursor).unwrap_or("");
    state.input_cursor = before_cursor.rfind('\n').map_or(0, |i| i.saturating_add(1));
    state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
}

/// Move cursor to the end of the current line.
fn move_end(state: &mut State) {
    let after_cursor = state.input.get(state.input_cursor..).unwrap_or("");
    state.input_cursor = state.input_cursor.saturating_add(after_cursor.find('\n').unwrap_or(after_cursor.len()));
    state.input_cursor = eject_cursor_from_sentinel(&state.input, state.input_cursor);
}

// ── Public handlers: non-selecting movement ──────────────────────────

/// Handle `CursorLeft` — move one character left, collapse selection if active.
pub(super) fn handle_cursor_left(state: &mut State) {
    if let Some(anchor) = state.input_selection_anchor {
        state.input_cursor = anchor.min(state.input_cursor);
        state.input_selection_anchor = None;
        return;
    }
    state.input_cursor = compute_char_left(&state.input, state.input_cursor);
}

/// Handle `CursorRight` — move one character right, collapse selection if active.
pub(super) fn handle_cursor_right(state: &mut State) {
    if let Some(anchor) = state.input_selection_anchor {
        state.input_cursor = anchor.max(state.input_cursor);
        state.input_selection_anchor = None;
        return;
    }
    state.input_cursor = compute_char_right(&state.input, state.input_cursor);
}

/// Handle `CursorWordLeft` — move to start of previous word, collapse selection if active.
pub(super) fn handle_cursor_word_left(state: &mut State) {
    if let Some(anchor) = state.input_selection_anchor {
        state.input_cursor = anchor.min(state.input_cursor);
        state.input_selection_anchor = None;
        return;
    }
    move_word_left(state);
}

/// Handle `CursorWordRight` — move to start of next word, collapse selection if active.
pub(super) fn handle_cursor_word_right(state: &mut State) {
    if let Some(anchor) = state.input_selection_anchor {
        state.input_cursor = anchor.max(state.input_cursor);
        state.input_selection_anchor = None;
        return;
    }
    move_word_right(state);
}

/// Handle `CursorHome` — move to beginning of current line, collapse selection if active.
pub(super) fn handle_cursor_home(state: &mut State) {
    if let Some(anchor) = state.input_selection_anchor {
        state.input_cursor = anchor.min(state.input_cursor);
        state.input_selection_anchor = None;
        return;
    }
    move_home(state);
}

/// Handle `CursorEnd` — move to end of current line, collapse selection if active.
pub(super) fn handle_cursor_end(state: &mut State) {
    if let Some(anchor) = state.input_selection_anchor {
        state.input_cursor = anchor.max(state.input_cursor);
        state.input_selection_anchor = None;
        return;
    }
    move_end(state);
}

// ── Public handlers: selecting movement (Shift+key) ──────────────────

/// Handle `CursorLeftSelect` — extend selection one character left.
pub(super) fn handle_cursor_left_select(state: &mut State) {
    extend_selection(state);
    state.input_cursor = compute_char_left(&state.input, state.input_cursor);
}

/// Handle `CursorRightSelect` — extend selection one character right.
pub(super) fn handle_cursor_right_select(state: &mut State) {
    extend_selection(state);
    state.input_cursor = compute_char_right(&state.input, state.input_cursor);
}

/// Handle `CursorWordLeftSelect` — extend selection one word left.
pub(super) fn handle_cursor_word_left_select(state: &mut State) {
    extend_selection(state);
    move_word_left(state);
}

/// Handle `CursorWordRightSelect` — extend selection one word right.
pub(super) fn handle_cursor_word_right_select(state: &mut State) {
    extend_selection(state);
    move_word_right(state);
}

/// Handle `CursorHomeSelect` — extend selection to start of line.
pub(super) fn handle_cursor_home_select(state: &mut State) {
    extend_selection(state);
    move_home(state);
}

/// Handle `CursorEndSelect` — extend selection to end of line.
pub(super) fn handle_cursor_end_select(state: &mut State) {
    extend_selection(state);
    move_end(state);
}

/// Handle `SelectAll` — select entire input.
pub(super) const fn handle_select_all(state: &mut State) {
    if state.input.is_empty() {
        return;
    }
    state.input_selection_anchor = Some(0);
    state.input_cursor = state.input.len();
}

// ── Existing helpers ─────────────────────────────────────────────────

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
        let cmd_content = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command)
            .iter()
            .find(|cmd| cmd.id == cmd_name)
            .map(|cmd| cmd.content.clone());
        if let Some(content) = cmd_content {
            let label = cmd_name.to_owned();
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
    // If selection active, delete selection instead
    if delete_selection(state) {
        return;
    }
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

/// Handle `DeleteWordLeft` — delete the word before the cursor.
pub(super) fn handle_delete_word_left(state: &mut State) {
    // If selection active, delete selection instead
    if delete_selection(state) {
        return;
    }
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
