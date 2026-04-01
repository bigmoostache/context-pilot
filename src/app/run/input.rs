use std::sync::mpsc::Sender;

use crossterm::event;

use crate::app::App;
use crate::app::actions::Action;
use crate::infra::gh_watcher::GhWatcher;
use crate::infra::watcher::FileWatcher;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::{build_message_op, build_save_batch};
use crate::state::{Message, State};
use crate::ui::TypewriterBuffer;
use crate::ui::help::CommandPalette;
use cp_base::panels::now_ms;

impl App {
    /// Create a new `App` with the given state, cache channel, and resume flag.
    pub(crate) fn new(state: State, cache_tx: Sender<CacheUpdate>, resume_stream: bool) -> Self {
        let file_watcher = FileWatcher::new().ok();
        let gh_watcher = GhWatcher::new(cache_tx.clone());

        Self {
            state,
            typewriter: TypewriterBuffer::new(),
            pending_done: None,
            pending_tools: Vec::new(),
            cache_tx,
            file_watcher,
            gh_watcher,
            watched_file_paths: std::collections::HashSet::new(),
            watched_dir_paths: std::collections::HashSet::new(),
            last_timer_check_ms: now_ms(),
            last_ownership_check_ms: now_ms(),
            pending_retry_error: None,
            last_render_ms: 0,
            last_spinner_ms: 0,
            last_gh_sync_ms: 0,
            last_chat_drain_ms: 0,
            api_check_rx: None,
            resume_stream,
            command_palette: CommandPalette::new(),
            wait_started_ms: 0,
            deferred_tool_sleep_until_ms: 0,
            deferred_tool_sleeping: false,
            writer: crate::state::persistence::PersistenceWriter::new(),
            last_poll_ms: std::collections::HashMap::new(),
            pending_question_tool_results: None,
            pending_console_wait_tool_results: None,
            accumulated_blocking_results: Vec::new(),
            reverie_streams: std::collections::HashMap::new(),
        }
    }

    /// Send state to background writer (debounced, non-blocking).
    /// Preferred over `save_state()` in the main event loop.
    pub(super) fn save_state_async(&self) {
        self.writer.send_batch(build_save_batch(&self.state));
    }

    /// Send a message to background writer (non-blocking).
    /// Preferred over `save_message()` in the main event loop.
    pub(super) fn save_message_async(&self, msg: &Message) {
        self.writer.send_message(build_message_op(msg));
    }

    /// Handle keyboard events when the @ autocomplete popup is active.
    /// Mutates `Suggestions` and state.input directly.
    pub(super) fn handle_autocomplete_event(&mut self, event: &event::Event) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let event::Event::Key(key) = event else { return };

        let Some(ac) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else { return };

        match key.code {
            KeyCode::Esc => {
                // Cancel: deactivate popup, leave @query text in input as-is
                ac.deactivate();
            }
            KeyCode::Up => {
                ac.select_prev();
            }
            KeyCode::Down => {
                ac.select_next();
            }
            KeyCode::Enter | KeyCode::Tab => {
                // Get the selected entry info
                let entry_info = ac.selected_match().map(|e| (e.name.clone(), e.is_dir));
                let Some((name, is_dir)) = entry_info else {
                    ac.deactivate();
                    return;
                };

                let full_path = ac.selected_full_path().unwrap_or(name);

                if is_dir {
                    // Folder: complete to "dir/" and show contents — don't close
                    let new_query = format!("{full_path}/");

                    // Update the input text: replace @<old_query> with @<new_query>
                    let anchor = ac.anchor_pos;
                    let old_cursor = self.state.input_cursor;
                    self.state.input = format!(
                        "{}@{}{}",
                        self.state.input.get(..anchor).unwrap_or(""),
                        new_query,
                        self.state.input.get(old_cursor..).unwrap_or("")
                    );
                    self.state.input_cursor = anchor.saturating_add(1).saturating_add(new_query.len()); // +1 for '@'

                    // Refresh entries for the new directory
                    let filter = cp_mod_tree::types::TreeState::get(&self.state).filter.clone();
                    let Some(ac_query) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else {
                        return;
                    };
                    ac_query.set_query(new_query);
                    let dir = ac_query.current_dir().to_string();
                    let prefix = ac_query.current_prefix().to_string();
                    let entries = cp_mod_tree::tools::list_dir_entries(&filter, &dir, &prefix);
                    let Some(ac_matches) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else {
                        return;
                    };
                    ac_matches.set_matches(entries);
                } else {
                    // File: insert the full path and close
                    let anchor = ac.anchor_pos;
                    ac.deactivate();
                    let cursor = self.state.input_cursor;
                    // Replace @<query> with the full file path (remove the @)
                    self.state.input = format!(
                        "{}{} {}",
                        self.state.input.get(..anchor).unwrap_or(""),
                        full_path,
                        self.state.input.get(cursor..).unwrap_or("")
                    );
                    self.state.input_cursor = anchor.saturating_add(full_path.len()).saturating_add(1); // +1 for space
                }
            }
            KeyCode::Backspace => {
                // Extract query info before re-borrowing
                let pop_result = ac.pop_char();
                let anchor = ac.anchor_pos;

                if pop_result {
                    let query_len = ac.query.len();
                    let query = ac.query.clone();
                    // Update cursor position to match shortened query
                    self.state.input_cursor = anchor.saturating_add(1).saturating_add(query_len); // +1 for '@'

                    // Also update the input text
                    let old_len = self.state.input.len();
                    let after_at = anchor.saturating_add(1); // skip '@'
                    // Rebuild: everything before @, then @query, then everything after old cursor
                    let rest_start = after_at.saturating_add(query.len()).saturating_add(1); // +1 for the removed char
                    if rest_start <= old_len {
                        self.state.input = format!(
                            "{}@{}{}",
                            self.state.input.get(..anchor).unwrap_or(""),
                            query,
                            self.state.input.get(rest_start..).unwrap_or("")
                        );
                    }

                    // Refresh matches
                    let filter = cp_mod_tree::types::TreeState::get(&self.state).filter.clone();
                    let Some(ac_dir) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else {
                        return;
                    };
                    let dir = ac_dir.current_dir().to_string();
                    let prefix = ac_dir.current_prefix().to_string();
                    let entries = cp_mod_tree::tools::list_dir_entries(&filter, &dir, &prefix);
                    let Some(ac_set) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else {
                        return;
                    };
                    ac_set.set_matches(entries);
                } else {
                    // Query was empty — remove the '@' and deactivate
                    ac.deactivate();
                    if anchor < self.state.input.len() {
                        let _r = self.state.input.remove(anchor);
                        self.state.input_cursor = anchor;
                    }
                }
            }
            KeyCode::Char(c) => {
                // Don't capture ctrl+key combos
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                // Space or newline cancels autocomplete
                if c == ' ' || c == '\n' {
                    ac.deactivate();
                    self.state.input.insert(self.state.input_cursor, c);
                    self.state.input_cursor = self.state.input_cursor.saturating_add(c.len_utf8());
                } else {
                    // Append to query and update input
                    ac.push_char(c);
                    self.state.input.insert(self.state.input_cursor, c);
                    self.state.input_cursor = self.state.input_cursor.saturating_add(c.len_utf8());

                    // Refresh matches with new query
                    let filter = cp_mod_tree::types::TreeState::get(&self.state).filter.clone();
                    let Some(ac_refresh) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else {
                        return;
                    };
                    let dir = ac_refresh.current_dir().to_string();
                    let prefix = ac_refresh.current_prefix().to_string();
                    let entries = cp_mod_tree::tools::list_dir_entries(&filter, &dir, &prefix);
                    let Some(ac_update) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else {
                        return;
                    };
                    ac_update.set_matches(entries);
                }
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Handle keyboard events when a question form is active.
    /// Mutates the `PendingForm` directly in state.
    pub(super) fn handle_question_form_event(&mut self, event: &event::Event) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let event::Event::Key(key) = event else { return };

        let Some(form) = self.state.get_ext_mut::<cp_base::ui::question_form::PendingForm>() else { return };

        // Check if currently typing in "Other" field
        let Some(current_answer) = form.answers.get(form.current_question) else { return };
        let typing_other = current_answer.typing_other;

        match key.code {
            KeyCode::Esc => {
                form.dismiss();
            }
            KeyCode::Up if !typing_other => {
                form.cursor_up();
            }
            KeyCode::Down if !typing_other => {
                form.cursor_down();
            }
            KeyCode::Left => {
                form.prev_question();
            }
            KeyCode::Right => {
                form.next_question();
            }
            KeyCode::Enter => {
                form.handle_enter();
            }
            KeyCode::Char(' ') if !typing_other && form.is_multi_select() => {
                form.toggle_selection();
            }
            KeyCode::Char(' ') if !typing_other => {
                // Single-select: space selects and advances
                form.toggle_selection();
            }
            // When on "Other": arrow keys navigate away, typing goes to text field
            KeyCode::Up if typing_other => {
                form.cursor_up();
            }
            KeyCode::Down if typing_other => {
                form.cursor_down();
            }
            KeyCode::Backspace if typing_other => {
                form.backspace();
            }
            KeyCode::Char(c) if typing_other => {
                // Don't capture ctrl+key combos
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    form.type_char(c);
                }
            }
            // Remaining keys do nothing in the question form
            KeyCode::Backspace
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Char(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Handle keyboard events when command palette is open
    pub(super) fn handle_palette_event(&mut self, event: &event::Event) -> Option<Action> {
        use crossterm::event::{KeyCode, KeyModifiers};

        let event::Event::Key(key) = event else {
            return Some(Action::None);
        };

        match key.code {
            // Escape closes palette
            KeyCode::Esc => {
                self.command_palette.close();
                None
            }
            // Enter executes selected command
            KeyCode::Enter => {
                if let Some(cmd) = self.command_palette.get_selected() {
                    let id = cmd.id.clone();
                    self.command_palette.close();

                    // Handle different command types
                    match id.as_str() {
                        "quit" => return None, // Signal quit
                        "reload" => {
                            self.state.flags.lifecycle.reload_pending = true;
                            return Some(Action::None);
                        }
                        "config" => return Some(Action::ToggleConfigView),
                        _ => {
                            // Navigate to any context panel (P-prefixed or special IDs like "chat")
                            if self.state.context.iter().any(|c| c.id == id) {
                                return Some(Action::SelectContextById(id));
                            }
                        }
                    }
                }
                Some(Action::None)
            }
            // Up/Down navigate results
            KeyCode::Up => {
                self.command_palette.select_prev();
                None
            }
            KeyCode::Down => {
                self.command_palette.select_next();
                None
            }
            // Left/Right move cursor in query
            KeyCode::Left => {
                self.command_palette.cursor_left();
                None
            }
            KeyCode::Right => {
                self.command_palette.cursor_right();
                None
            }
            // Home/End for cursor
            KeyCode::Home => {
                self.command_palette.cursor = 0;
                None
            }
            KeyCode::End => {
                self.command_palette.cursor = self.command_palette.query.len();
                None
            }
            // Backspace/Delete
            KeyCode::Backspace => {
                self.command_palette.backspace(&self.state);
                None
            }
            KeyCode::Delete => {
                self.command_palette.delete(&self.state);
                None
            }
            // Character input
            KeyCode::Char(c) => {
                // Ignore Ctrl+char combinations
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return None;
                }
                self.command_palette.insert_char(c, &self.state);
                None
            }
            // Tab could cycle through results
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.command_palette.select_prev();
                } else {
                    self.command_palette.select_next();
                }
                None
            }
            KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::BackTab
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => None,
        }
    }
}
