//! File path autocomplete triggered by `@` in the input field.
//!
//! Works like shell tab-completion:
//! - Shows entries (files + folders) in the current directory
//! - Prefix-matches the partial name typed after the last `
//! - Tab on a folder → completes to `folder/` and shows its contents
//! - Tab on a file → inserts the full path and closes
//!
//! Stored in `State.module_data` via the TypeMap pattern (get_ext/set_ext).

/// Maximum number of matches to display in the autocomplete popup.
const MAX_VISIBLE: usize = 10;

/// A single entry in the autocomplete list.
#[derive(Debug, Clone)]
pub struct AutocompleteEntry {
    /// Display name (just the file/folder name, not the full path).
    pub name: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
}

/// State for the @-triggered file path autocomplete popup.
#[derive(Debug, Clone)]
pub struct AutocompleteState {
    /// Whether the autocomplete popup is currently visible.
    pub active: bool,
    /// Byte position of the '@' character in state.input.
    pub anchor_pos: usize,
    /// The full query text typed after '@' (e.g., "src/ui/m").
    pub query: String,
    /// The directory portion of the query (e.g., "src/ui" for query "src/ui/m").
    pub dir_prefix: String,
    /// The partial name being matched (e.g., "m" for query "src/ui/m").
    pub name_prefix: String,
    /// Entries in the current directory that match the prefix.
    pub matches: Vec<AutocompleteEntry>,
    /// Index of the currently highlighted match (0-based).
    pub selected: usize,
    /// Scroll offset for the visible window into matches.
    pub scroll_offset: usize,
    /// Number of visual lines the input area occupies (set by conversation panel render).
    /// Used to position the popup just above the input field.
    pub input_visual_lines: u16,
}

impl Default for AutocompleteState {
    fn default() -> Self {
        Self::new()
    }
}

impl AutocompleteState {
    /// Create a new inactive autocomplete state with all fields zeroed.
    pub fn new() -> Self {
        Self {
            active: false,
            anchor_pos: 0,
            query: String::new(),
            dir_prefix: String::new(),
            name_prefix: String::new(),
            matches: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            input_visual_lines: 2,
        }
    }

    /// Activate autocomplete at the given anchor position.
    /// Caller must call `set_matches()` afterward to populate entries.
    pub fn activate(&mut self, anchor_pos: usize) {
        self.active = true;
        self.anchor_pos = anchor_pos;
        self.query.clear();
        self.dir_prefix.clear();
        self.name_prefix.clear();
        self.matches.clear();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Deactivate and reset the autocomplete popup.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.query.clear();
        self.dir_prefix.clear();
        self.name_prefix.clear();
        self.matches.clear();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Append a character to the query. Caller must call `set_matches()` afterward.
    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.split_query();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Remove the last character from the query.
    /// Returns false if query was already empty (caller should deactivate).
    /// Caller must call `set_matches()` afterward if true is returned.
    pub fn pop_char(&mut self) -> bool {
        if self.query.pop().is_some() {
            self.split_query();
            self.selected = 0;
            self.scroll_offset = 0;
            true
        } else {
            false
        }
    }

    /// Set the query to a new value (used when completing into a folder).
    /// Caller must call `set_matches()` afterward.
    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.split_query();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Replace the current match list with new entries.
    pub fn set_matches(&mut self, entries: Vec<AutocompleteEntry>) {
        self.matches = entries;
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if !self.matches.is_empty() && self.selected < self.matches.len() - 1 {
            self.selected += 1;
            if self.selected >= self.scroll_offset + MAX_VISIBLE {
                self.scroll_offset = self.selected + 1 - MAX_VISIBLE;
            }
        }
    }

    /// Get the currently selected match, if any.
    pub fn selected_match(&self) -> Option<&AutocompleteEntry> {
        self.matches.get(self.selected)
    }

    /// Build the full path for the selected entry.
    pub fn selected_full_path(&self) -> Option<String> {
        self.selected_match().map(|entry| {
            if self.dir_prefix.is_empty() { entry.name.clone() } else { format!("{}/{}", self.dir_prefix, entry.name) }
        })
    }

    /// The visible window of matches for rendering.
    pub fn visible_matches(&self) -> &[AutocompleteEntry] {
        let end = (self.scroll_offset + MAX_VISIBLE).min(self.matches.len());
        &self.matches[self.scroll_offset..end]
    }

    /// Get the directory prefix for the current query.
    pub fn current_dir(&self) -> &str {
        &self.dir_prefix
    }

    /// Get the partial name being matched.
    pub fn current_prefix(&self) -> &str {
        &self.name_prefix
    }

    /// Split query into `dir_prefix` and `name_prefix` at the last '/'.
    fn split_query(&mut self) {
        if let Some(last_slash) = self.query.rfind('/') {
            self.dir_prefix = self.query[..last_slash].to_string();
            self.name_prefix = self.query[last_slash + 1..].to_string();
        } else {
            self.dir_prefix.clear();
            self.name_prefix = self.query.clone();
        }
    }
}
