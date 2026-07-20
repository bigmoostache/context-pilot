use std::sync::mpsc::Sender;

use crossterm::event;

use crate::app::App;
use crate::app::actions::Action;
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

        Self {
            state,
            typewriter: TypewriterBuffer::new(),
            pending_done: None,
            pending_tools: Vec::new(),
            cache_tx,
            file_watcher,
            watched_file_paths: std::collections::HashSet::new(),
            watched_dir_paths: std::collections::HashSet::new(),
            last_timer_check_ms: now_ms(),
            last_ownership_check_ms: now_ms(),
            pending_retry_error: None,
            last_render_ms: 0,
            last_spinner_ms: 0,
            last_bridge_recover_ms: 0,
            last_chat_drain_ms: 0,
            api_check_rx: None,
            resume_stream,
            command_palette: CommandPalette::new(),
            wait_started_ms: 0,
            deferred_tool_sleep_until_ms: 0,
            deferred_tool_sleeping: false,
            writer: crate::state::persistence::PersistenceWriter::new(),
            last_poll_ms: std::collections::HashMap::new(),
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
        let &event::Event::Key(key) = event else { return };
        if self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>().is_none() {
            return;
        }

        match key.code {
            // Cancel: deactivate popup, leave @query text in input as-is.
            KeyCode::Esc => self.autocomplete_with(cp_base::state::autocomplete::Suggestions::deactivate),
            KeyCode::Up => self.autocomplete_with(cp_base::state::autocomplete::Suggestions::select_prev),
            KeyCode::Down => self.autocomplete_with(cp_base::state::autocomplete::Suggestions::select_next),
            KeyCode::Enter | KeyCode::Tab => self.autocomplete_accept(),
            KeyCode::Backspace => self.autocomplete_backspace(),
            KeyCode::Char(c) => {
                // Don't capture ctrl+key combos.
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.autocomplete_char(c);
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

    /// Run `f` against the live `Suggestions` popup (no-op if torn down). Used
    /// for the pure-navigation arms (deactivate / select prev / select next).
    fn autocomplete_with(&mut self, f: fn(&mut cp_base::state::autocomplete::Suggestions)) {
        if let Some(ac) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() {
            f(ac);
        }
    }

    /// Re-fetch the `Suggestions` popup and repopulate its match list for its
    /// current directory + prefix (shared tail of every query-mutating arm).
    /// No-op if the popup was torn down between borrows.
    fn autocomplete_refresh_matches(&mut self) {
        let filter = cp_mod_tree::types::TreeState::get(&self.state).filter.clone();
        let Some(ac) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else { return };
        let dir = ac.current_dir().to_owned();
        let prefix = ac.current_prefix().to_owned();
        let entries = cp_mod_tree::tools::list_dir_entries(&filter, &dir, &prefix);
        let Some(ac_set) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else { return };
        ac_set.set_matches(entries);
    }

    /// Accept the selected autocomplete entry (Enter/Tab): a directory completes
    /// to `dir/` and refreshes contents (popup stays open); a file inserts its
    /// full path plus a trailing space and closes the popup.
    fn autocomplete_accept(&mut self) {
        let Some(ac) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else { return };
        let entry_info = ac.selected_match().map(|e| (e.name.clone(), e.is_dir));
        let Some((name, is_dir)) = entry_info else {
            ac.deactivate();
            return;
        };
        let full_path = ac.selected_full_path().unwrap_or(name);
        let anchor = ac.anchor_pos;

        if is_dir {
            // Folder: complete to "dir/" and show contents — don't close.
            let new_query = format!("{full_path}/");
            let old_cursor = self.state.input_cursor;
            self.state.input = format!(
                "{}@{}{}",
                self.state.input.get(..anchor).unwrap_or(""),
                new_query,
                self.state.input.get(old_cursor..).unwrap_or("")
            );
            self.state.input_cursor = anchor.saturating_add(1).saturating_add(new_query.len()); // +1 for '@'
            if let Some(ac_query) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() {
                ac_query.set_query(new_query);
            }
            self.autocomplete_refresh_matches();
        } else {
            // File: insert the full path and close.
            ac.deactivate();
            let cursor = self.state.input_cursor;
            self.state.input = format!(
                "{}{} {}",
                self.state.input.get(..anchor).unwrap_or(""),
                full_path,
                self.state.input.get(cursor..).unwrap_or("")
            );
            self.state.input_cursor = anchor.saturating_add(full_path.len()).saturating_add(1); // +1 for space
        }
    }

    /// Backspace inside the autocomplete popup: shorten the `@query` (refreshing
    /// matches), or — when the query is already empty — remove the `@` sentinel
    /// and close the popup.
    fn autocomplete_backspace(&mut self) {
        let Some(ac) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else { return };
        let pop_result = ac.pop_char();
        let anchor = ac.anchor_pos;

        if pop_result {
            let query = ac.query.clone();
            // Update cursor position to match shortened query.
            self.state.input_cursor = anchor.saturating_add(1).saturating_add(query.len()); // +1 for '@'

            // Rebuild input: before @, then @query, then everything past old cursor.
            let old_len = self.state.input.len();
            let after_at = anchor.saturating_add(1); // skip '@'
            let rest_start = after_at.saturating_add(query.len()).saturating_add(1); // +1 for removed char
            if rest_start <= old_len {
                self.state.input = format!(
                    "{}@{}{}",
                    self.state.input.get(..anchor).unwrap_or(""),
                    query,
                    self.state.input.get(rest_start..).unwrap_or("")
                );
            }
            self.autocomplete_refresh_matches();
        } else {
            // Query was empty — remove the '@' and deactivate.
            ac.deactivate();
            if anchor < self.state.input.len() {
                let _r = self.state.input.remove(anchor);
                self.state.input_cursor = anchor;
            }
        }
    }

    /// Type a character into the autocomplete popup: a space/newline cancels it
    /// (inserting the char literally); any other char extends the `@query` and
    /// refreshes matches.
    fn autocomplete_char(&mut self, c: char) {
        let Some(ac) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() else { return };
        if c == ' ' || c == '\n' {
            ac.deactivate();
            self.state.input.insert(self.state.input_cursor, c);
            self.state.input_cursor = self.state.input_cursor.saturating_add(c.len_utf8());
        } else {
            ac.push_char(c);
            self.state.input.insert(self.state.input_cursor, c);
            self.state.input_cursor = self.state.input_cursor.saturating_add(c.len_utf8());
            self.autocomplete_refresh_matches();
        }
    }

    /// Handle keyboard events when command palette is open
    pub(super) fn handle_palette_event(&mut self, event: &event::Event) -> Option<Action> {
        use crossterm::event::KeyCode;

        let &event::Event::Key(key) = event else {
            return Some(Action::None);
        };

        // Escape closes the palette; Enter executes the selection. Every other
        // key drives query editing / result navigation (handled exhaustively in
        // `palette_edit_nav`, so no wildcard match arm here).
        if key.code == KeyCode::Esc {
            self.command_palette.close();
            return None;
        }
        if key.code == KeyCode::Enter {
            return self.palette_execute_selected();
        }
        self.palette_edit_nav(key);
        None
    }

    /// Query-editing + result-navigation keys for the command palette (every
    /// key except Esc/Enter): arrows/Home/End move the selection or cursor,
    /// Backspace/Delete/Char edit the query, Tab cycles results. Ignores
    /// Ctrl+char combos and inert keys.
    fn palette_edit_nav(&mut self, key: event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match key.code {
            KeyCode::Up => self.command_palette.select_prev(),
            KeyCode::Down => self.command_palette.select_next(),
            KeyCode::Left => self.command_palette.cursor_left(),
            KeyCode::Right => self.command_palette.cursor_right(),
            KeyCode::Home => self.command_palette.cursor = 0,
            KeyCode::End => self.command_palette.cursor = self.command_palette.query.len(),
            KeyCode::Backspace => self.command_palette.backspace(&self.state),
            KeyCode::Delete => self.command_palette.delete(&self.state),
            KeyCode::Char(c) => {
                // Ignore Ctrl+char combinations.
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.command_palette.insert_char(c, &self.state);
                }
            }
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.command_palette.select_prev();
                } else {
                    self.command_palette.select_next();
                }
            }
            KeyCode::Esc
            | KeyCode::Enter
            | KeyCode::PageUp
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
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Execute the palette's selected command (Enter): close the palette, then
    /// dispatch by command id — `quit` signals quit (`None`), `reload` sets the
    /// reload flag, `config` toggles the config view, and any context-panel id
    /// navigates to that panel. Unknown ids are a no-op (`Action::None`).
    fn palette_execute_selected(&mut self) -> Option<Action> {
        let Some(cmd) = self.command_palette.get_selected() else {
            return Some(Action::None);
        };
        let id = cmd.id.clone();
        self.command_palette.close();

        match id.as_str() {
            "quit" => None, // Signal quit
            "reload" => {
                self.state.flags.lifecycle.reload_pending = true;
                Some(Action::None)
            }
            "config" => Some(Action::ToggleConfigView),
            _ => {
                // Navigate to any context panel (P-prefixed or special IDs like "chat").
                if self.state.context.iter().any(|c| c.id == id) {
                    Some(Action::SelectContextById(id))
                } else {
                    Some(Action::None)
                }
            }
        }
    }
}
