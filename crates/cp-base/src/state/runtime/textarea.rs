//! Shared text-editing engine for every TUI textarea.
//!
//! [`TextArea`] owns one editable buffer plus its cursor, selection anchor, and
//! an undo ring. It is the single place where selection, clipboard, and undo
//! semantics live, so the composer and every auxiliary field (e.g. the
//! new-thread title) share identical behaviour instead of re-implementing it.
//!
//! Scope boundary: the pure buffer/cursor/selection/undo operations live here.
//! Paste-sentinel (`\x00{idx}\x00`) and `/command` expansion remain in the
//! composer's action layer because they depend on per-thread paste buffers —
//! this engine stays sentinel-agnostic and reusable by plain-text fields.

use std::collections::VecDeque;
use std::io::Write as _;

use serde::{Deserialize, Serialize};

/// Maximum number of undo snapshots retained. Snapshots coalesce consecutive
/// inserts into one group, so this is ~10 edit *groups*, not 10 keystrokes.
const UNDO_CAP: usize = 10;

/// Classifies the last mutation so consecutive inserts coalesce into a single
/// undo group (typing a word is one undo, not one-per-character). Transient —
/// never serialized.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind {
    /// A character/text insertion (coalesces with the previous insert).
    Insert,
    /// A deletion (backspace, delete, word-delete, selection delete).
    Delete,
    /// Any other mutation (paste, selection replace) — never coalesces.
    Other,
}

/// One editable text buffer with cursor, optional selection, and undo history.
///
/// Byte offsets (`cursor`, `anchor`) always sit on UTF-8 char boundaries; the
/// action layer guarantees this. The selection, when active, spans
/// `min(anchor, cursor)..max(anchor, cursor)`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TextArea {
    /// The editable text.
    pub text: String,
    /// Cursor position as a byte offset into `text`.
    pub cursor: usize,
    /// Selection anchor (byte offset); `Some` while a selection is active.
    /// Transient — a reload starts with no selection.
    #[serde(skip)]
    pub anchor: Option<usize>,
    /// Undo ring of past `(text, cursor)` states, newest at the back. Capped at
    /// [`UNDO_CAP`] groups and wiped on send. Transient across reloads.
    #[serde(skip)]
    pub undo: VecDeque<(String, usize)>,
    /// Kind of the most recent mutation, for insert coalescing. Transient.
    #[serde(skip)]
    pub last_kind: Option<EditKind>,
}

impl TextArea {
    // ── Selection ────────────────────────────────────────────────────

    /// Ordered selection range `(start, end)` if a non-collapsed selection is
    /// active, else `None`. A collapsed anchor (== cursor) yields `None`.
    #[must_use]
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        let (start, end) = (anchor.min(self.cursor), anchor.max(self.cursor));
        (start != end).then_some((start, end))
    }

    /// The currently selected text, if any.
    #[must_use]
    pub fn selected_text(&self) -> Option<&str> {
        let (start, end) = self.selection_range()?;
        self.text.get(start..end)
    }

    /// Drop any active selection (cursor unchanged).
    pub const fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Start a selection at the cursor if none is active (for Shift+movement).
    pub const fn extend_anchor(&mut self) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
    }

    /// Select the whole buffer (anchor at start, cursor at end). No-op when
    /// empty.
    pub const fn select_all(&mut self) {
        if self.text.is_empty() {
            return;
        }
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// Delete the active selection and collapse the cursor to its start.
    /// Returns `true` when a non-empty selection was removed. Records an undo
    /// snapshot first so the deletion is reversible.
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            self.anchor = None;
            return false;
        };
        self.push_undo(EditKind::Delete);
        self.text = format!("{}{}", self.text.get(..start).unwrap_or(""), self.text.get(end..).unwrap_or(""));
        self.cursor = start;
        self.anchor = None;
        true
    }

    /// Replace the active selection (or insert at the cursor when none) with
    /// `s`, leaving the cursor just past the inserted text. Records undo.
    pub fn replace_selection(&mut self, s: &str) {
        self.push_undo(EditKind::Other);
        let at = if let Some((start, end)) = self.selection_range() {
            self.text = format!("{}{}", self.text.get(..start).unwrap_or(""), self.text.get(end..).unwrap_or(""));
            start
        } else {
            self.cursor
        };
        self.anchor = None;
        self.text.insert_str(at, s);
        self.cursor = at.saturating_add(s.len());
    }

    // ── Undo ─────────────────────────────────────────────────────────

    /// Record the current `(text, cursor)` as an undo snapshot before a
    /// mutation of the given `kind`. Consecutive [`EditKind::Insert`] groups
    /// coalesce (no new snapshot), so typing a run of characters is a single
    /// undo step. The ring is capped at [`UNDO_CAP`]; the oldest is dropped
    /// when full.
    pub fn push_undo(&mut self, kind: EditKind) {
        if kind == EditKind::Insert && self.last_kind == Some(EditKind::Insert) {
            self.last_kind = Some(kind);
            return;
        }
        self.last_kind = Some(kind);
        self.undo.push_back((self.text.clone(), self.cursor));
        while self.undo.len() > UNDO_CAP {
            let _dropped = self.undo.pop_front();
        }
    }

    /// Revert to the most recent undo snapshot. Returns `true` when a state was
    /// restored, `false` when the ring was empty. Clears any selection.
    pub fn undo(&mut self) -> bool {
        let Some((text, cursor)) = self.undo.pop_back() else {
            return false;
        };
        self.text = text;
        self.cursor = cursor.min(self.text.len());
        self.anchor = None;
        self.last_kind = None;
        true
    }

    /// Discard all undo history and reset coalescing. Called on send/clear so a
    /// fresh message starts with no reachable past states.
    pub fn clear_undo(&mut self) {
        self.undo.clear();
        self.last_kind = None;
    }

    // ── Buffer reset ─────────────────────────────────────────────────

    /// Clear the buffer, cursor, selection, and undo history in one shot
    /// (used when a message is submitted).
    pub fn reset(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
        self.clear_undo();
    }

    // ── System clipboard ─────────────────────────────────────────────

    /// Copy the current selection to the system clipboard via `pbcopy`.
    /// Returns `true` when a non-empty selection was copied. No-op (returns
    /// `false`) when there is no selection or `pbcopy` is unavailable.
    #[must_use]
    pub fn copy_selection_to_clipboard(&self) -> bool {
        let Some(sel) = self.selected_text().filter(|s| !s.is_empty()) else {
            return false;
        };
        copy_to_clipboard(sel)
    }
}

/// Write `text` to the system clipboard via `pbcopy`. Returns `true` on a
/// successful spawn+write. Shared helper so copy paths don't duplicate the
/// process plumbing.
#[must_use]
pub fn copy_to_clipboard(text: &str) -> bool {
    let Ok(mut child) = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _r = stdin.write_all(text.as_bytes());
    }
    child.wait().is_ok()
}

/// Read the system clipboard via `pbpaste`. Returns the clipboard text, or an
/// empty string when `pbpaste` is unavailable or fails.
#[must_use]
pub fn read_clipboard() -> String {
    std::process::Command::new("pbpaste")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_all_spans_buffer() {
        let mut ta = TextArea { text: "hello".to_owned(), cursor: 2, ..Default::default() };
        ta.select_all();
        assert_eq!(ta.selection_range(), Some((0, 5)));
        assert_eq!(ta.selected_text(), Some("hello"));
    }

    #[test]
    fn select_all_empty_is_noop() {
        let mut ta = TextArea::default();
        ta.select_all();
        assert_eq!(ta.selection_range(), None);
    }

    #[test]
    fn collapsed_selection_is_none() {
        let ta = TextArea { text: "abc".to_owned(), cursor: 1, anchor: Some(1), ..Default::default() };
        assert_eq!(ta.selection_range(), None);
        assert_eq!(ta.selected_text(), None);
    }

    #[test]
    fn delete_selection_removes_and_collapses() {
        let mut ta = TextArea { text: "hello world".to_owned(), cursor: 5, anchor: Some(0), ..Default::default() };
        assert!(ta.delete_selection());
        assert_eq!(ta.text, " world");
        assert_eq!(ta.cursor, 0);
        assert_eq!(ta.anchor, None);
    }

    #[test]
    fn replace_selection_swaps_text() {
        let mut ta = TextArea { text: "hello world".to_owned(), cursor: 5, anchor: Some(0), ..Default::default() };
        ta.replace_selection("bye");
        assert_eq!(ta.text, "bye world");
        assert_eq!(ta.cursor, 3);
    }

    #[test]
    fn replace_without_selection_inserts_at_cursor() {
        let mut ta = TextArea { text: "ad".to_owned(), cursor: 1, ..Default::default() };
        ta.replace_selection("bc");
        assert_eq!(ta.text, "abcd");
        assert_eq!(ta.cursor, 3);
    }

    #[test]
    fn inserts_coalesce_into_one_undo_group() {
        let mut ta = TextArea::default();
        for ch in "abc".chars() {
            ta.push_undo(EditKind::Insert);
            ta.text.push(ch);
            ta.cursor = ta.text.len();
        }
        // Three inserts coalesced → one snapshot (the empty pre-typing state).
        assert!(ta.undo());
        assert_eq!(ta.text, "");
        assert!(!ta.undo());
    }

    #[test]
    fn distinct_edits_get_distinct_snapshots() {
        let mut ta = TextArea::default();
        ta.push_undo(EditKind::Insert);
        ta.text = "a".to_owned();
        ta.cursor = 1;
        ta.push_undo(EditKind::Delete);
        ta.text = "ab".to_owned();
        ta.cursor = 2;
        // Undo delete-group → back to "a"; undo insert-group → back to "".
        assert!(ta.undo());
        assert_eq!(ta.text, "a");
        assert!(ta.undo());
        assert_eq!(ta.text, "");
    }

    #[test]
    fn undo_ring_is_capped() {
        let mut ta = TextArea::default();
        // 15 distinct delete-groups; only the last UNDO_CAP are retained.
        for i in 0u32..15 {
            ta.push_undo(EditKind::Delete);
            ta.text = format!("state{i}");
            ta.cursor = ta.text.len();
        }
        let mut count = 0;
        while ta.undo() {
            count += 1;
        }
        assert_eq!(count, UNDO_CAP);
    }

    #[test]
    fn clear_undo_wipes_history() {
        let mut ta = TextArea::default();
        ta.push_undo(EditKind::Insert);
        ta.text = "x".to_owned();
        ta.clear_undo();
        assert!(!ta.undo());
    }

    #[test]
    fn selection_cleared_after_undo() {
        let mut ta = TextArea { text: "abc".to_owned(), cursor: 3, anchor: Some(0), ..Default::default() };
        ta.push_undo(EditKind::Delete);
        ta.text = "bc".to_owned();
        ta.cursor = 0;
        assert!(ta.undo());
        assert_eq!(ta.anchor, None, "undo clears selection");
    }
}
