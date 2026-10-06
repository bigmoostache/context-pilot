//! Prompt history navigation and panel clipboard copy.

use crate::state::persistence::message::load_prompt_history;
use crate::state::{Kind, State};

/// Prompt history navigation state (stored in `State`'s `TypeMap`).
pub(crate) struct PromptHistoryNav {
    /// Past prompts loaded from `prompt-history.jsonl`, oldest first.
    entries: Vec<String>,
    /// Current position. `None` = not navigating (showing draft).
    index: Option<usize>,
    /// Saved input text when navigation started.
    draft: String,
    /// Whether the JSONL file has been loaded yet (lazy init).
    loaded: bool,
}

impl PromptHistoryNav {
    /// Create an empty, unloaded navigation state.
    const fn new() -> Self {
        Self { entries: Vec::new(), index: None, draft: String::new(), loaded: false }
    }

    /// Push a new entry to the history.
    pub(super) fn push(&mut self, entry: String) {
        self.entries.push(entry);
    }

    /// Reset navigation state after submission.
    pub(super) fn reset_nav(&mut self) {
        self.index = None;
        self.draft.clear();
    }
}

/// Ensure `PromptHistoryNav` exists in the type-map; lazy-load entries on first call.
pub(super) fn ensure_history_nav(state: &mut State) {
    if state.get_ext::<PromptHistoryNav>().is_none() {
        state.set_ext(PromptHistoryNav::new());
    }
    let nav = state.ext_mut::<PromptHistoryNav>();
    if !nav.loaded {
        nav.entries = load_prompt_history();
        nav.loaded = true;
    }
}

/// Navigate to the previous (older) prompt in history (Ctrl+U).
pub(super) fn handle_history_prev(state: &mut State) {
    ensure_history_nav(state);
    // Clone input before mutable borrow of TypeMap
    let current_input = state.composer.text.clone();
    let nav = state.ext_mut::<PromptHistoryNav>();
    if nav.entries.is_empty() {
        return;
    }
    let new_text = match nav.index {
        None => {
            // Start navigating — save current input as draft
            nav.draft = current_input;
            let idx = nav.entries.len().saturating_sub(1);
            nav.index = Some(idx);
            match nav.entries.get(idx) {
                Some(entry) => entry.clone(),
                None => return,
            }
        }
        Some(idx) if idx > 0 => {
            let prev = idx.saturating_sub(1);
            nav.index = Some(prev);
            match nav.entries.get(prev) {
                Some(entry) => entry.clone(),
                None => return,
            }
        }
        Some(_) => return, // Already at oldest entry
    };
    state.composer.text = new_text;
    state.composer.cursor = state.composer.text.len();
    state.composer.anchor = None;
}

/// Navigate to the next (newer) prompt in history (Ctrl+D).
pub(super) fn handle_history_next(state: &mut State) {
    if state.get_ext::<PromptHistoryNav>().is_none() {
        return;
    }
    let nav = state.ext_mut::<PromptHistoryNav>();
    let Some(idx) = nav.index else { return };
    let next = idx.saturating_add(1);
    if next < nav.entries.len() {
        nav.index = Some(next);
        let text = match nav.entries.get(next) {
            Some(entry) => entry.clone(),
            None => return,
        };
        state.composer.text = text;
    } else {
        // Back to the draft (current unsaved input)
        let draft = nav.draft.clone();
        nav.index = None;
        state.composer.text = draft;
    }
    state.composer.cursor = state.composer.text.len();
    state.composer.anchor = None;
}

/// Copy the current panel's content to the system clipboard (Ctrl+C).
pub(super) fn handle_copy_panel_content(state: &mut State) {
    use std::io::Write as _;

    let Some(context_type) = state.context.get(state.selected_context).map(|c| c.context_type.clone()) else {
        return;
    };
    let is_conversation = context_type.as_str() == Kind::CONVERSATION;
    let panel = crate::app::panels::get_panel(&context_type);
    let items = panel.context(state);
    let mut text: String = items.iter().map(|i| i.content.as_str()).collect::<Vec<_>>().join("\n\n");

    // If on conversation panel, append the pending input
    if is_conversation && !state.composer.text.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&state.composer.text);
    }

    if text.is_empty() {
        return;
    }

    // Copy via pbcopy (macOS)
    if let Ok(mut child) = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _r = stdin.write_all(text.as_bytes());
        }
        let _r = child.wait();
    }
    // Visual feedback via status bar flash
    state.flags.overlays.copied_flash_ms = crate::app::panels::now_ms();
    state.flags.ui.dirty = true;
}

/// Copy the active composer selection to the system clipboard (Ctrl+C while a
/// selection is active). Wins over [`handle_copy_panel_content`] per the
/// selection-priority rule. No-op when there is no selection.
pub(super) fn handle_copy_selection(state: &mut State) {
    if state.composer.copy_selection_to_clipboard() {
        state.flags.overlays.copied_flash_ms = crate::app::panels::now_ms();
        state.flags.ui.dirty = true;
    }
}
