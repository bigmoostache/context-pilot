use crossterm::event;

use crate::app::actions::Action;
use crate::infra::tools::perform_reload;

use crate::app::App;

impl App {
    /// Handle keyboard events when the @ autocomplete popup is active.
    /// Mutates `AutocompleteState` and state.input directly.
    pub(super) fn handle_autocomplete_event(&mut self, event: &event::Event) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let event::Event::Key(key) = event else { return };

        let Some(ac) = self.state.get_ext_mut::<cp_base::autocomplete::AutocompleteState>() else { return };

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

                if is_dir {
                    // Folder: complete to "dir/" and show contents — don't close
                    let full_path = ac.selected_full_path().unwrap_or(name);
                    let new_query = format!("{full_path}/");

                    // Update the input text: replace @<old_query> with @<new_query>
                    let anchor = ac.anchor_pos;
                    let old_cursor = self.state.input_cursor;
                    self.state.input =
                        format!("{}@{}{}", &self.state.input[..anchor], new_query, &self.state.input[old_cursor..]);
                    self.state.input_cursor = anchor + 1 + new_query.len(); // +1 for '@'

                    // Refresh entries for the new directory
                    let filter = cp_mod_tree::TreeState::get(&self.state).tree_filter.clone();
                    let ac = self.state.get_ext_mut::<cp_base::autocomplete::AutocompleteState>().unwrap();
                    ac.set_query(new_query);
                    let dir = ac.current_dir().to_string();
                    let prefix = ac.current_prefix().to_string();
                    let entries = cp_mod_tree::list_dir_entries(&filter, &dir, &prefix);
                    let ac = self.state.get_ext_mut::<cp_base::autocomplete::AutocompleteState>().unwrap();
                    ac.set_matches(entries);
                } else {
                    // File: insert the full path and close
                    let full_path = ac.selected_full_path().unwrap_or(name);
                    let anchor = ac.anchor_pos;
                    ac.deactivate();
                    let cursor = self.state.input_cursor;
                    // Replace @<query> with the full file path (remove the @)
                    self.state.input =
                        format!("{}{} {}", &self.state.input[..anchor], full_path, &self.state.input[cursor..]);
                    self.state.input_cursor = anchor + full_path.len() + 1; // +1 for space
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
                    self.state.input_cursor = anchor + 1 + query_len; // +1 for '@'

                    // Also update the input text
                    let old_len = self.state.input.len();
                    let after_at = anchor + 1; // skip '@'
                    // Rebuild: everything before @, then @query, then everything after old cursor
                    let rest_start = after_at + query.len() + 1; // +1 for the removed char
                    if rest_start <= old_len {
                        self.state.input =
                            format!("{}@{}{}", &self.state.input[..anchor], query, &self.state.input[rest_start..]);
                    }

                    // Refresh matches
                    let filter = cp_mod_tree::TreeState::get(&self.state).tree_filter.clone();
                    let ac = self.state.get_ext_mut::<cp_base::autocomplete::AutocompleteState>().unwrap();
                    let dir = ac.current_dir().to_string();
                    let prefix = ac.current_prefix().to_string();
                    let entries = cp_mod_tree::list_dir_entries(&filter, &dir, &prefix);
                    let ac = self.state.get_ext_mut::<cp_base::autocomplete::AutocompleteState>().unwrap();
                    ac.set_matches(entries);
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
                    self.state.input_cursor += c.len_utf8();
                } else {
                    // Append to query and update input
                    ac.push_char(c);
                    self.state.input.insert(self.state.input_cursor, c);
                    self.state.input_cursor += c.len_utf8();

                    // Refresh matches with new query
                    let filter = cp_mod_tree::TreeState::get(&self.state).tree_filter.clone();
                    let ac = self.state.get_ext_mut::<cp_base::autocomplete::AutocompleteState>().unwrap();
                    let dir = ac.current_dir().to_string();
                    let prefix = ac.current_prefix().to_string();
                    let entries = cp_mod_tree::list_dir_entries(&filter, &dir, &prefix);
                    let ac = self.state.get_ext_mut::<cp_base::autocomplete::AutocompleteState>().unwrap();
                    ac.set_matches(entries);
                }
            }
            _ => {}
        }
    }

    /// Handle keyboard events when a question form is active.
    /// Mutates the `PendingQuestionForm` directly in state.
    pub(super) fn handle_question_form_event(&mut self, event: &event::Event) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let event::Event::Key(key) = event else { return };

        let Some(form) = self.state.get_ext_mut::<cp_base::ui::PendingQuestionForm>() else { return };

        // Check if currently typing in "Other" field
        let typing_other = form.answers[form.current_question].typing_other;

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
            // Non-typing-other: any char that's not space does nothing
            _ => {}
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
                            // Perform reload (sets reload_requested flag and exits)
                            perform_reload(&mut self.state);
                            return None; // Won't reach here, but needed for type system
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
            _ => None,
        }
    }
}
