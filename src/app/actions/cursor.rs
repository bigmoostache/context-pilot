//! Cursor movement, text editing, selection management, and command expansion.
//!
//! The pure selection/undo primitives live on [`TextArea`](cp_base::state::runtime::textarea::TextArea)
//! (the shared engine). This module holds the composer-specific logic that the
//! engine deliberately stays out of: paste-sentinel (`\x00{idx}\x00`) skipping /
//! removal and `/command` expansion, which depend on the per-thread paste
//! buffers. Field access goes through `state.composer` (the resident thread's
//! [`TextArea`]).

use super::helpers::eject_cursor_from_sentinel;
use crate::state::State;
use cp_base::state::runtime::textarea::EditKind;

// ── Selection helpers (delegate to the shared engine) ────────────────

/// Delete selected text and collapse cursor to selection start.
/// Returns `true` if there was a non-empty selection that was deleted.
pub(super) fn delete_selection(state: &mut State) -> bool {
    state.thread_mut().composer.delete_selection()
}

/// Ensure a selection anchor is set (for Shift+movement).
fn extend_selection(state: &mut State) {
    state.thread_mut().composer.extend_anchor();
}

// ── Sentinel detection ───────────────────────────────────────────────

/// `pos` sits on a `\x00`: try to read it as the OPENING marker of a sentinel
/// (`\x00{digits}\x00`). Returns the `(start, end)` span when it is.
fn sentinel_from_opening(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    let mut end = pos.saturating_add(1);
    while end < bytes.len() && bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end = end.saturating_add(1);
    }
    if end > pos.saturating_add(1) && bytes.get(end) == Some(&0) {
        return Some((pos, end.saturating_add(1)));
    }
    None
}

/// `pos` sits on a `\x00`: try to read it as the CLOSING marker of a sentinel
/// (digits preceded by an opening `\x00`). Returns the `(start, end)` span.
fn sentinel_from_closing(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos == 0 {
        return None;
    }
    let mut start = pos;
    while start > 0 && bytes.get(start.saturating_sub(1)).is_some_and(u8::is_ascii_digit) {
        start = start.saturating_sub(1);
    }
    if start < pos && start > 0 && bytes.get(start.saturating_sub(1)) == Some(&0) {
        return Some((start.saturating_sub(1), pos.saturating_add(1)));
    }
    None
}

/// `pos` sits on a digit: scan both directions to see if it is inside a
/// sentinel's digit run. Returns the enclosing `(start, end)` span.
fn sentinel_from_digit(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    let mut start = pos;
    while start > 0 && bytes.get(start.saturating_sub(1)).is_some_and(u8::is_ascii_digit) {
        start = start.saturating_sub(1);
    }
    if start == 0 || bytes.get(start.saturating_sub(1)) != Some(&0) {
        return None;
    }
    let mut end = pos.saturating_add(1);
    while end < bytes.len() && bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end = end.saturating_add(1);
    }
    if bytes.get(end) == Some(&0) {
        return Some((start.saturating_sub(1), end.saturating_add(1)));
    }
    None
}

/// Find the paste sentinel (\x00{digits}\x00) that contains byte position `pos`, if any.
/// Returns `(start, end)` where `start` is the opening \x00 position and `end` is one past
/// the closing \x00.
fn find_enclosing_sentinel(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos >= bytes.len() {
        return None;
    }
    let &b = bytes.get(pos)?;
    if b == 0 {
        sentinel_from_opening(bytes, pos).or_else(|| sentinel_from_closing(bytes, pos))
    } else if b.is_ascii_digit() {
        sentinel_from_digit(bytes, pos)
    } else {
        None
    }
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
    let composer = &mut state.thread_mut().composer;
    if composer.cursor > 0 {
        let before = composer.text.get(..composer.cursor).unwrap_or("");
        let trimmed = before.trim_end();
        composer.cursor = if trimmed.is_empty() {
            0
        } else {
            trimmed.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i.saturating_add(1))
        };
        composer.cursor = eject_cursor_from_sentinel(&composer.text, composer.cursor);
    }
}

/// Move cursor to the start of the next word.
fn move_word_right(state: &mut State) {
    let composer = &mut state.thread_mut().composer;
    if composer.cursor < composer.text.len() {
        let after = composer.text.get(composer.cursor..).unwrap_or("");
        let skip_word = after.find(|c: char| c.is_whitespace()).unwrap_or(after.len());
        let remaining = after.get(skip_word..).unwrap_or("");
        let skip_space = remaining.find(|c: char| !c.is_whitespace()).unwrap_or(remaining.len());
        composer.cursor = composer.cursor.saturating_add(skip_word.saturating_add(skip_space));
        composer.cursor = eject_cursor_from_sentinel(&composer.text, composer.cursor);
    }
}

/// Move cursor to the beginning of the current line.
fn move_home(state: &mut State) {
    let composer = &mut state.thread_mut().composer;
    let before_cursor = composer.text.get(..composer.cursor).unwrap_or("");
    composer.cursor = before_cursor.rfind('\n').map_or(0, |i| i.saturating_add(1));
    composer.cursor = eject_cursor_from_sentinel(&composer.text, composer.cursor);
}

/// Move cursor to the end of the current line.
fn move_end(state: &mut State) {
    let composer = &mut state.thread_mut().composer;
    let after_cursor = composer.text.get(composer.cursor..).unwrap_or("");
    composer.cursor = composer.cursor.saturating_add(after_cursor.find('\n').unwrap_or(after_cursor.len()));
    composer.cursor = eject_cursor_from_sentinel(&composer.text, composer.cursor);
}

// ── Public handlers: non-selecting movement ──────────────────────────

/// Handle `CursorLeft` — move one character left, collapse selection if active.
pub(super) fn handle_cursor_left(state: &mut State) {
    if let Some((start, _)) = state.thread().composer.selection_range() {
        state.thread_mut().composer.cursor = start;
        state.thread_mut().composer.clear_selection();
        return;
    }
    state.thread_mut().composer.clear_selection();
    let composer = &mut state.thread_mut().composer;
    composer.cursor = compute_char_left(&composer.text, composer.cursor);
}

/// Handle `CursorRight` — move one character right, collapse selection if active.
pub(super) fn handle_cursor_right(state: &mut State) {
    if let Some((_, end)) = state.thread().composer.selection_range() {
        state.thread_mut().composer.cursor = end;
        state.thread_mut().composer.clear_selection();
        return;
    }
    state.thread_mut().composer.clear_selection();
    let composer = &mut state.thread_mut().composer;
    composer.cursor = compute_char_right(&composer.text, composer.cursor);
}

/// Handle `CursorWordLeft` — move to start of previous word, collapse selection if active.
pub(super) fn handle_cursor_word_left(state: &mut State) {
    if let Some((start, _)) = state.thread().composer.selection_range() {
        state.thread_mut().composer.cursor = start;
        state.thread_mut().composer.clear_selection();
        return;
    }
    state.thread_mut().composer.clear_selection();
    move_word_left(state);
}

/// Handle `CursorWordRight` — move to start of next word, collapse selection if active.
pub(super) fn handle_cursor_word_right(state: &mut State) {
    if let Some((_, end)) = state.thread().composer.selection_range() {
        state.thread_mut().composer.cursor = end;
        state.thread_mut().composer.clear_selection();
        return;
    }
    state.thread_mut().composer.clear_selection();
    move_word_right(state);
}

/// Handle `CursorHome` — move to beginning of current line, collapse selection if active.
pub(super) fn handle_cursor_home(state: &mut State) {
    if let Some((start, _)) = state.thread().composer.selection_range() {
        state.thread_mut().composer.cursor = start;
        state.thread_mut().composer.clear_selection();
        return;
    }
    state.thread_mut().composer.clear_selection();
    move_home(state);
}

/// Handle `CursorEnd` — move to end of current line, collapse selection if active.
pub(super) fn handle_cursor_end(state: &mut State) {
    if let Some((_, end)) = state.thread().composer.selection_range() {
        state.thread_mut().composer.cursor = end;
        state.thread_mut().composer.clear_selection();
        return;
    }
    state.thread_mut().composer.clear_selection();
    move_end(state);
}

// ── Public handlers: selecting movement (Shift+key) ──────────────────

/// Handle `CursorLeftSelect` — extend selection one character left.
pub(super) fn handle_cursor_left_select(state: &mut State) {
    extend_selection(state);
    let composer = &mut state.thread_mut().composer;
    composer.cursor = compute_char_left(&composer.text, composer.cursor);
}

/// Handle `CursorRightSelect` — extend selection one character right.
pub(super) fn handle_cursor_right_select(state: &mut State) {
    extend_selection(state);
    let composer = &mut state.thread_mut().composer;
    composer.cursor = compute_char_right(&composer.text, composer.cursor);
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

/// Handle `SelectAll` — select entire input (Ctrl+A).
pub(super) fn handle_select_all(state: &mut State) {
    state.thread_mut().composer.select_all();
}

/// Handle `Undo` — revert the composer to its previous snapshot (Ctrl+Z).
pub(super) fn handle_undo(state: &mut State) {
    let _reverted = state.thread_mut().composer.undo();
}

// ── Existing helpers ─────────────────────────────────────────────────

/// Handle `/command` expansion after typing space or newline.
pub(super) fn handle_command_expansion(state: &mut State) {
    // Find start of current "word" — scan back past the space we just inserted
    let before_space = state.thread().composer.cursor.saturating_sub(1); // position of the space
    let bytes = state.thread().composer.text.as_bytes();
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
    while word_start < before_space && !state.thread().composer.text.is_char_boundary(word_start) {
        word_start = word_start.saturating_add(1);
    }
    let word = state.thread().composer.text.get(word_start..before_space).unwrap_or("");
    if let Some(cmd_name) = word.strip_prefix('/') {
        let cmd_content = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command)
            .iter()
            .find(|cmd| cmd.id == cmd_name)
            .map(|cmd| cmd.content.clone());
        if let Some(content) = cmd_content {
            let label = cmd_name.to_owned();
            let idx = state.thread().paste_buffers.len();
            state.thread_mut().paste_buffers.push(content);
            state.thread_mut().paste_buffer_labels.push(Some(label));
            let sentinel = format!("\x00{idx}\x00");
            // Replace /command<space> with sentinel
            state.thread_mut().composer.text = format!(
                "{}{}\n{}",
                state.thread().composer.text.get(..word_start).unwrap_or(""),
                sentinel,
                state.thread().composer.text.get(state.thread().composer.cursor..).unwrap_or(""),
            );
            state.thread_mut().composer.cursor = word_start.saturating_add(sentinel.len()).saturating_add(1);
        }
    }
}

/// Cursor is just past a closing `\x00`: remove the whole sentinel by scanning
/// back to the opening `\x00`. Returns `true` when a sentinel was removed.
fn backspace_closing_sentinel(state: &mut State) -> bool {
    let bytes = state.thread().composer.text.as_bytes();
    let mut scan = state.thread().composer.cursor.saturating_sub(2); // skip closing \x00
    while let Some(&b) = bytes.get(scan) {
        if b == 0 || scan == 0 {
            break;
        }
        scan = scan.saturating_sub(1);
    }
    if bytes.get(scan) != Some(&0) {
        return false;
    }
    state.thread_mut().composer.text = format!(
        "{}{}",
        state.thread().composer.text.get(..scan).unwrap_or(""),
        state.thread().composer.text.get(state.thread().composer.cursor..).unwrap_or("")
    );
    state.thread_mut().composer.cursor = scan;
    true
}

/// Cursor sits on a digit that may be inside a sentinel: if so, remove the whole
/// sentinel. Returns `true` when a sentinel was removed, `false` to fall through
/// to a normal backspace.
fn backspace_digit_sentinel(state: &mut State, cursor_prev: usize) -> bool {
    let bytes = state.thread().composer.text.as_bytes();
    let mut scan = cursor_prev;
    while let Some(&b) = bytes.get(scan) {
        if !b.is_ascii_digit() || scan == 0 {
            break;
        }
        scan = scan.saturating_sub(1);
    }
    if bytes.get(scan) != Some(&0) {
        return false;
    }
    // Inside a sentinel — find the closing \x00.
    let mut end = state.thread().composer.cursor;
    while let Some(&b) = bytes.get(end) {
        if b == 0 {
            break;
        }
        end = end.saturating_add(1);
    }
    if bytes.get(end) == Some(&0) {
        end = end.saturating_add(1); // include closing \x00
    }
    state.thread_mut().composer.text = format!(
        "{}{}",
        state.thread().composer.text.get(..scan).unwrap_or(""),
        state.thread().composer.text.get(end..).unwrap_or("")
    );
    state.thread_mut().composer.cursor = scan;
    true
}

/// Handle backspace, including paste sentinel removal.
pub(super) fn handle_input_backspace(state: &mut State) {
    // If selection active, delete selection instead (records its own undo).
    if delete_selection(state) {
        return;
    }
    if state.thread().composer.cursor == 0 {
        return;
    }
    state.thread_mut().composer.push_undo(EditKind::Delete);
    let cursor_prev = state.thread().composer.cursor.saturating_sub(1);
    let Some(&prev_b) = state.thread().composer.text.as_bytes().get(cursor_prev) else { return };

    if prev_b == 0 {
        if !backspace_closing_sentinel(state) {
            normal_backspace(state);
        }
    } else if state.thread().composer.cursor >= 2 && prev_b.is_ascii_digit() {
        if !backspace_digit_sentinel(state, cursor_prev) {
            normal_backspace(state);
        }
    } else {
        normal_backspace(state);
    }
}

/// Remove one character before the cursor (normal backspace).
fn normal_backspace(state: &mut State) {
    let prev = state
        .thread()
        .composer
        .text
        .get(..state.thread().composer.cursor)
        .unwrap_or("")
        .char_indices()
        .last()
        .map_or(0, |(i, _)| i);
    let _r = state.thread_mut().composer.text.remove(prev);
    state.thread_mut().composer.cursor = prev;
}

/// Handle `DeleteWordLeft` — delete the word before the cursor.
pub(super) fn handle_delete_word_left(state: &mut State) {
    // If selection active, delete selection instead (records its own undo).
    if delete_selection(state) {
        return;
    }
    if state.thread().composer.cursor > 0 {
        state.thread_mut().composer.push_undo(EditKind::Delete);
        let before = state.thread().composer.text.get(..state.thread().composer.cursor).unwrap_or("");
        let trimmed = before.trim_end();
        let word_start = if trimmed.is_empty() {
            0
        } else {
            trimmed.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i.saturating_add(1))
        };
        state.thread_mut().composer.text = format!(
            "{}{}",
            state.thread().composer.text.get(..word_start).unwrap_or(""),
            state.thread().composer.text.get(state.thread().composer.cursor..).unwrap_or("")
        );
        state.thread_mut().composer.cursor = word_start;
    }
}

/// Handle `RemoveListItem` — delete from line start to cursor.
pub(super) fn handle_remove_list_item(state: &mut State) {
    if state.thread().composer.cursor > 0 {
        state.thread_mut().composer.push_undo(EditKind::Delete);
        let before = state.thread().composer.text.get(..state.thread().composer.cursor).unwrap_or("");
        let line_start = before.rfind('\n').map_or(0, |i| i.saturating_add(1));
        state.thread_mut().composer.text = format!(
            "{}{}",
            state.thread().composer.text.get(..line_start).unwrap_or(""),
            state.thread().composer.text.get(state.thread().composer.cursor..).unwrap_or("")
        );
        state.thread_mut().composer.cursor = line_start;
    }
}
