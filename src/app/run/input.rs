use std::sync::mpsc::Sender;

use crossterm::event;

use crate::app::App;
use crate::app::actions::Action;
use crate::infra::watcher::FileWatcher;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::{build_message_op, build_save_batch};
use crate::state::{Message, State};
use crate::ui::help::CommandPalette;
use cp_base::panels::now_ms;

impl App {
    /// Create a new `App` with the given state, cache channel, and resume flag.
    pub(crate) fn new(state: State, cache_tx: Sender<CacheUpdate>, resume_stream: bool) -> Self {
        let file_watcher = FileWatcher::new().ok();

        Self {
            state,
            cache_tx,
            file_watcher,
            watched_file_paths: std::collections::HashSet::new(),
            watched_dir_paths: std::collections::HashSet::new(),
            watch_specs_hash: 0,
            last_timer_check_ms: now_ms(),
            last_ownership_check_ms: now_ms(),
            last_render_ms: 0,
            last_full_redraw_ms: now_ms(),

            last_spinner_ms: 0,
            last_bridge_recover_ms: 0,
            last_chat_drain_ms: 0,
            api_check_rx: None,
            resume_stream,
            command_palette: CommandPalette::new(),
            writer: crate::state::persistence::PersistenceWriter::new(),
            last_poll_ms: std::collections::HashMap::new(),
            reverie_streams: std::collections::HashMap::new(),
            thread_streams: std::collections::HashMap::new(),
            fleet: cp_fleet::FleetRegistry::new(),
            stepping_thread: None,
            input_ready: None,
        }
    }

    /// Key identifying the executing thread — the channel key used for both stream
    /// *spawn* and stream *drain*, so the two can never diverge.
    ///
    /// During a background advancement step this is
    /// [`stepping_thread`](crate::app::App::stepping_thread); otherwise it is the
    /// focused thread (or [`DEFAULT_WORKER_ID`](crate::infra::constants::DEFAULT_WORKER_ID)
    /// before any thread is focused). Deriving the key from the same source at
    /// spawn and drain time is what keeps each thread's stream events on its own
    /// bundle once several threads advance concurrently.
    pub(super) fn executing_key(&self) -> String {
        if let Some(id) = self.stepping_thread.as_ref() {
            return id.clone();
        }
        cp_mod_threads::types::FocusState::get(&self.state)
            .focused_thread_id
            .clone()
            .unwrap_or_else(|| crate::infra::constants::DEFAULT_WORKER_ID.to_owned())
    }

    /// Start an LLM stream for the executing thread over a fresh per-thread
    /// channel, storing its receiver in
    /// [`thread_streams`](crate::app::App::thread_streams) for the loop to drain.
    ///
    /// This replaces the former single app-wide stream channel: each stream now
    /// owns its mpsc (mirroring reverie streams), so concurrent threads can each
    /// have a live stream. The channel is keyed by
    /// [`executing_key`](Self::executing_key) — the thread currently in `state` —
    /// so a background thread's stream lands under its own id, never colliding
    /// with the focused thread's.
    pub(super) fn spawn_thread_stream(&mut self, params: crate::llms::StreamParams) {
        let (tx, rx) = std::sync::mpsc::channel();
        crate::infra::api::start_streaming(params, tx);
        let key = self.executing_key();
        let _prev = self.thread_streams.insert(key, crate::app::ThreadStream { rx });
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
    /// Mutates `Suggestions` and state.composer.text directly.
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
            let old_cursor = self.state.thread().composer.cursor;
            self.state.thread_mut().composer.text = format!(
                "{}@{}{}",
                self.state.thread().composer.text.get(..anchor).unwrap_or(""),
                new_query,
                self.state.thread().composer.text.get(old_cursor..).unwrap_or("")
            );
            self.state.thread_mut().composer.cursor = anchor.saturating_add(1).saturating_add(new_query.len()); // +1 for '@'
            if let Some(ac_query) = self.state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() {
                ac_query.set_query(new_query);
            }
            self.autocomplete_refresh_matches();
        } else {
            // File: insert the full path and close.
            ac.deactivate();
            let cursor = self.state.thread().composer.cursor;
            self.state.thread_mut().composer.text = format!(
                "{}{} {}",
                self.state.thread().composer.text.get(..anchor).unwrap_or(""),
                full_path,
                self.state.thread().composer.text.get(cursor..).unwrap_or("")
            );
            self.state.thread_mut().composer.cursor = anchor.saturating_add(full_path.len()).saturating_add(1); // +1 for space
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
            self.state.thread_mut().composer.cursor = anchor.saturating_add(1).saturating_add(query.len()); // +1 for '@'

            // Rebuild input: before @, then @query, then everything past old cursor.
            let old_len = self.state.thread().composer.text.len();
            let after_at = anchor.saturating_add(1); // skip '@'
            let rest_start = after_at.saturating_add(query.len()).saturating_add(1); // +1 for removed char
            if rest_start <= old_len {
                self.state.thread_mut().composer.text = format!(
                    "{}@{}{}",
                    self.state.thread().composer.text.get(..anchor).unwrap_or(""),
                    query,
                    self.state.thread().composer.text.get(rest_start..).unwrap_or("")
                );
            }
            self.autocomplete_refresh_matches();
        } else {
            // Query was empty — remove the '@' and deactivate.
            ac.deactivate();
            if anchor < self.state.thread().composer.text.len() {
                let _r = self.state.thread_mut().composer.text.remove(anchor);
                self.state.thread_mut().composer.cursor = anchor;
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
            let pos = self.state.thread().composer.cursor;
            self.state.thread_mut().composer.text.insert(pos, c);
            self.state.thread_mut().composer.cursor =
                self.state.thread_mut().composer.cursor.saturating_add(c.len_utf8());
        } else {
            ac.push_char(c);
            let pos = self.state.thread().composer.cursor;
            self.state.thread_mut().composer.text.insert(pos, c);
            self.state.thread_mut().composer.cursor =
                self.state.thread_mut().composer.cursor.saturating_add(c.len_utf8());
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
                if self.state.thread().context.iter().any(|c| c.id == id) {
                    Some(Action::SelectContextById(id))
                } else {
                    Some(Action::None)
                }
            }
        }
    }
}
