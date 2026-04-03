//! Input handling — maps egui events to application actions.
//!
//! Provides [`InputState`] for tracking text input, focus, and keyboard
//! shortcuts. The egui `TextEdit` widget handles most text editing
//! natively (cursor, selection, clipboard). This module adds
//! application-level keybindings on top.

use eframe::egui;

// ── Input state ──────────────────────────────────────────────────────

/// Tracks the user's text input and focus state.
#[derive(Debug)]
pub struct State {
    /// Current input buffer (what the user is typing).
    pub text: String,
    /// Whether the text input area should grab focus this frame.
    pub request_focus: bool,
    /// History of submitted messages (for up-arrow recall).
    pub history: Vec<String>,
    /// Current position in history navigation (`None` = fresh input).
    pub history_index: Option<usize>,
    /// Saved draft when navigating history (restored on Escape).
    pub draft: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            text: String::new(),
            request_focus: true,
            history: Vec::new(),
            history_index: None,
            draft: String::new(),
        }
    }
}

impl State {
    /// Submit the current input text. Returns the submitted text,
    /// or `None` if the input was empty/whitespace.
    pub fn submit(&mut self) -> Option<String> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let submitted = self.text.clone();
        self.history.push(submitted.clone());
        self.text.clear();
        self.history_index = None;
        self.draft.clear();
        Some(submitted)
    }

    /// Navigate to the previous history entry (up arrow).
    pub fn history_back(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.draft.clone_from(&self.text);
                self.history_index = Some(self.history.len().saturating_sub(1));
            }
            Some(idx) if idx > 0 => {
                self.history_index = Some(idx.saturating_sub(1));
            }
            _ => return,
        }
        if let Some(idx) = self.history_index
            && let Some(entry) = self.history.get(idx)
        {
            self.text.clone_from(entry);
        }
    }

    /// Navigate to the next history entry (down arrow).
    pub fn history_forward(&mut self) {
        match self.history_index {
            None => {}
            Some(idx) => {
                let last = self.history.len().saturating_sub(1);
                if idx >= last {
                    // Back to draft.
                    self.history_index = None;
                    self.text.clone_from(&self.draft);
                } else {
                    let next = idx.saturating_add(1);
                    self.history_index = Some(next);
                    if let Some(entry) = self.history.get(next) {
                        self.text.clone_from(entry);
                    }
                }
            }
        }
    }

    /// Clear input and reset history navigation.
    pub fn clear(&mut self) {
        self.text.clear();
        self.history_index = None;
        self.draft.clear();
    }

    /// Number of submitted messages in history.
    #[must_use]
    pub const fn history_len(&self) -> usize {
        self.history.len()
    }
}

// ── Keyboard shortcut detection ──────────────────────────────────────

/// Application-level actions triggered by keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    /// Submit the current input (Enter without Shift).
    Submit,
    /// Clear the input field (Escape).
    ClearInput,
    /// Navigate input history backwards (Up arrow when input focused).
    HistoryBack,
    /// Navigate input history forwards (Down arrow when input focused).
    HistoryForward,
    /// Cycle to the next sidebar panel (Tab).
    NextPanel,
    /// Cycle to the previous sidebar panel (Shift+Tab).
    PreviousPanel,
    /// Jump to sidebar panel by index (Ctrl+1..9).
    JumpToPanel(u8),
    /// Toggle sidebar mode (Ctrl+B).
    ToggleSidebar,
    /// Open help overlay (Ctrl+H or F1).
    ToggleHelp,
}

/// Poll egui input events and return any triggered [`AppAction`]s.
///
/// Called once per frame from `App::update()`. Only returns actions
/// from keys that are *not* consumed by a focused `TextEdit`.
#[must_use]
pub fn poll_actions(ctx: &egui::Context) -> Vec<AppAction> {
    let mut actions = Vec::new();

    ctx.input(|input| {
        // Ctrl+B → toggle sidebar.
        if input.modifiers.ctrl && input.key_pressed(egui::Key::B) {
            actions.push(AppAction::ToggleSidebar);
        }

        // Ctrl+H or F1 → help.
        if input.modifiers.ctrl && input.key_pressed(egui::Key::H) {
            actions.push(AppAction::ToggleHelp);
        }

        // Ctrl+1..9 → jump to panel.
        for (key, idx) in [
            (egui::Key::Num1, 1),
            (egui::Key::Num2, 2),
            (egui::Key::Num3, 3),
            (egui::Key::Num4, 4),
            (egui::Key::Num5, 5),
            (egui::Key::Num6, 6),
            (egui::Key::Num7, 7),
            (egui::Key::Num8, 8),
            (egui::Key::Num9, 9),
        ] {
            if input.modifiers.ctrl && input.key_pressed(key) {
                actions.push(AppAction::JumpToPanel(idx));
            }
        }

        // Tab / Shift+Tab → cycle panels (only when no text widget focused).
        if input.key_pressed(egui::Key::Tab) && !input.modifiers.ctrl {
            if input.modifiers.shift {
                actions.push(AppAction::PreviousPanel);
            } else {
                actions.push(AppAction::NextPanel);
            }
        }
    });

    actions
}
