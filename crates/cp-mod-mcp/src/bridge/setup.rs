//! Mutable form state for the MCP setup overlay.
//!
//! Stored as a `State` extension (`state.set_ext(McpSetupState::default())`),
//! alongside [`McpState`](super::servers::McpState). The builder reads both
//! to produce the rendering IR
//! ([`McpSetupOverlay`](cp_render::mcp_overlay_ir::McpSetupOverlay)).

use cp_base::state::runtime::State;

use super::form_enums::{AuthMode, Scope, ServerType};

// ── Top-level overlay state ─────────────────────────────────────────────────

/// Mutable state for the MCP setup overlay, stored as a `State` extension.
#[derive(Debug, Clone)]
pub struct McpSetupState {
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Index of the highlighted server in the list.
    pub selected_index: usize,
    /// Current interaction mode.
    pub mode: Mode,
    /// Active add/edit form (present in `AddForm` / `EditForm` modes).
    pub form: Option<Form>,
    /// Transient error message (cleared on next action).
    pub error: Option<String>,
    /// Transient success message (cleared on next action).
    pub success: Option<String>,
    /// Server name pending deletion (set in `ConfirmDelete` mode).
    pub delete_target: Option<String>,
}

impl Default for McpSetupState {
    fn default() -> Self {
        Self {
            visible: false,
            selected_index: 0,
            mode: Mode::List,
            form: None,
            error: None,
            success: None,
            delete_target: None,
        }
    }
}

impl McpSetupState {
    /// Shared ref from the `State` extension map.
    ///
    /// # Panics
    ///
    /// Panics if the module's `init_state` never ran (extension absent).
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Mutable ref from the `State` extension map.
    ///
    /// # Panics
    ///
    /// Panics if the module's `init_state` never ran (extension absent).
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Toggle overlay visibility. Resets to list mode when opening.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.mode = Mode::List;
            self.form = None;
            self.error = None;
            self.success = None;
        }
    }

    /// Clear transient messages (called before each action).
    pub fn clear_messages(&mut self) {
        self.error = None;
        self.success = None;
    }

    /// Enter add-server mode with a blank form.
    pub fn start_add(&mut self) {
        self.clear_messages();
        self.mode = Mode::AddForm;
        self.form = Some(Form::default());
    }

    /// Enter confirm-delete mode for the given server name.
    pub fn start_delete(&mut self, server_name: String) {
        self.clear_messages();
        self.mode = Mode::ConfirmDelete;
        self.delete_target = Some(server_name);
    }

    /// Return to list mode, clearing form and delete target.
    pub fn cancel(&mut self) {
        self.mode = Mode::List;
        self.form = None;
        self.delete_target = None;
    }
}

// ── Interaction mode ────────────────────────────────────────────────────────

/// Current overlay interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing the server list.
    List,
    /// Filling out the add-server form.
    AddForm,
    /// Confirming deletion of a server.
    ConfirmDelete,
    /// OAuth flow in progress.
    OAuthPending,
}

// ── Form state ──────────────────────────────────────────────────────────────

/// Mutable form for adding a new server.
#[derive(Debug, Clone)]
pub struct Form {
    /// Server name (key in `mcp.json`).
    pub name: String,
    /// Server type: stdio or http.
    pub server_type: ServerType,
    /// Command to spawn (stdio only).
    pub command: String,
    /// Space-separated arguments (stdio only). Split on save.
    pub args: String,
    /// Remote URL (http only).
    pub url: String,
    /// Bearer token (http + bearer auth only).
    pub bearer_token: String,
    /// Authentication mode.
    pub auth_mode: AuthMode,
    /// Config scope: global or project.
    pub scope: Scope,
    /// Index of the currently focused form field.
    pub focused_field: usize,
    /// Cursor position within the focused text field.
    pub cursor_pos: usize,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            name: String::new(),
            server_type: ServerType::Stdio,
            command: String::new(),
            args: String::new(),
            url: String::new(),
            bearer_token: String::new(),
            auth_mode: AuthMode::None,
            scope: Scope::Project,
            focused_field: 0,
            cursor_pos: 0,
        }
    }
}

impl Form {
    /// Total number of navigable fields, varies by server type and auth mode.
    #[must_use]
    pub fn field_count(&self) -> usize {
        // Fields: name, server_type, (command+args | url+bearer_token+auth_mode), scope
        match self.server_type {
            ServerType::Stdio => {
                // name, server_type, command, args, scope = 5
                5
            }
            ServerType::Http => {
                // name, server_type, url, auth_mode, (bearer_token if Bearer), scope
                if self.auth_mode == AuthMode::Bearer {
                    6 // name, server_type, url, auth_mode, bearer_token, scope
                } else {
                    5 // name, server_type, url, auth_mode, scope
                }
            }
        }
    }

    /// Get the text value of the currently focused field (for text input).
    /// Returns `None` for selector fields (`server_type`, `auth_mode`, `scope`).
    #[must_use]
    pub fn focused_text(&self) -> Option<&str> {
        let f = self.field_at(self.focused_field)?;
        f.text_value(self)
    }

    /// Get the field descriptor at the given index.
    #[must_use]
    pub fn field_at(&self, index: usize) -> Option<FormField> {
        let fields = self.visible_fields();
        fields.into_iter().nth(index)
    }

    /// Ordered list of visible fields given current `server_type` and `auth_mode`.
    #[must_use]
    pub fn visible_fields(&self) -> Vec<FormField> {
        let mut fields = vec![FormField::Name, FormField::ServerType];
        match self.server_type {
            ServerType::Stdio => {
                fields.push(FormField::Command);
                fields.push(FormField::Args);
            }
            ServerType::Http => {
                fields.push(FormField::Url);
                fields.push(FormField::AuthMode);
                if self.auth_mode == AuthMode::Bearer {
                    fields.push(FormField::BearerToken);
                }
            }
        }
        fields.push(FormField::Scope);
        fields
    }

    /// Clamp `focused_field` to valid range after field visibility changes.
    pub fn clamp_focus(&mut self) {
        let max = self.field_count().saturating_sub(1);
        if self.focused_field > max {
            self.focused_field = max;
        }
        self.cursor_pos = self.current_text_len();
    }

    /// Move focus to the next field (wraps around).
    pub fn focus_next(&mut self) {
        let count = self.field_count();
        if count > 0 {
            let next = self.focused_field.saturating_add(1);
            self.focused_field = if next >= count { 0 } else { next };
        }
        self.cursor_pos = self.current_text_len();
    }

    /// Move focus to the previous field (wraps around).
    pub fn focus_prev(&mut self) {
        let count = self.field_count();
        if count > 0 {
            self.focused_field = if self.focused_field == 0 {
                count.saturating_sub(1)
            } else {
                self.focused_field.saturating_sub(1)
            };
        }
        self.cursor_pos = self.current_text_len();
    }

    /// Insert a character at the cursor position in the focused text field.
    pub fn insert_char(&mut self, c: char) {
        let cursor = self.cursor_pos;
        let Some(field) = self.field_at(self.focused_field) else { return };
        let inserted = field.text_value_mut(self).is_some_and(|text| {
            let byte_pos = text
                .char_indices()
                .nth(cursor)
                .map_or(text.len(), |(i, _)| i);
            text.insert(byte_pos, c);
            true
        });
        if inserted {
            self.cursor_pos = cursor.saturating_add(1);
        }
    }

    /// Delete the character before the cursor in the focused text field.
    pub fn delete_char(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let cursor = self.cursor_pos;
        let Some(field) = self.field_at(self.focused_field) else { return };
        let deleted = field.text_value_mut(self).is_some_and(|text| {
            text.char_indices().nth(cursor.saturating_sub(1)).map(|(i, _)| i).is_some_and(|pos| {
                let _c = text.remove(pos);
                true
            })
        });
        if deleted {
            self.cursor_pos = cursor.saturating_sub(1);
        }
    }

    /// Move the cursor one character left in the focused text field.
    pub const fn move_cursor_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
    }

    /// Move the cursor one character right in the focused text field.
    pub fn move_cursor_right(&mut self) {
        let len = self.current_text_len();
        if self.cursor_pos < len {
            self.cursor_pos = self.cursor_pos.saturating_add(1);
        }
    }

    /// Move the cursor to the start of the focused text field.
    pub const fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    /// Move the cursor to the end of the focused text field.
    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.current_text_len();
    }

    /// Length (in chars) of the currently focused text field, or 0 for selectors.
    fn current_text_len(&self) -> usize {
        self.focused_text().map_or(0, |t| t.chars().count())
    }
}

// ── Form field descriptors ──────────────────────────────────────────────────

/// Identifies a form field by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    /// Server name (key in `mcp.json`).
    Name,
    /// Server type selector (stdio / http).
    ServerType,
    /// Command to spawn (stdio servers).
    Command,
    /// Space-separated arguments (stdio servers).
    Args,
    /// Remote URL (http servers).
    Url,
    /// Static bearer token (http + bearer auth).
    BearerToken,
    /// Authentication mode selector.
    AuthMode,
    /// Config scope selector (global / project).
    Scope,
}

impl FormField {
    /// Display label for this field.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Server Name",
            Self::ServerType => "Type",
            Self::Command => "Command",
            Self::Args => "Arguments",
            Self::Url => "URL",
            Self::BearerToken => "Bearer Token",
            Self::AuthMode => "Auth Mode",
            Self::Scope => "Scope",
        }
    }

    /// Placeholder text for text fields, `None` for selectors.
    #[must_use]
    pub const fn placeholder(self) -> Option<&'static str> {
        match self {
            Self::Name => Some("e.g. filesystem"),
            Self::Command => Some("e.g. npx"),
            Self::Args => Some("e.g. -y @modelcontextprotocol/server-filesystem ."),
            Self::Url => Some("e.g. https://mcp.example.com/mcp"),
            Self::BearerToken => Some("paste token here"),
            Self::ServerType | Self::AuthMode | Self::Scope => None,
        }
    }

    /// Whether this field is a text input (vs a selector).
    #[must_use]
    pub const fn is_text(self) -> bool {
        matches!(
            self,
            Self::Name | Self::Command | Self::Args | Self::Url | Self::BearerToken
        )
    }

    /// Get the text value for a text field from the form.
    #[must_use]
    fn text_value(self, form: &Form) -> Option<&str> {
        match self {
            Self::Name => Some(&form.name),
            Self::Command => Some(&form.command),
            Self::Args => Some(&form.args),
            Self::Url => Some(&form.url),
            Self::BearerToken => Some(&form.bearer_token),
            Self::ServerType | Self::AuthMode | Self::Scope => None,
        }
    }

    /// Get a mutable ref to the text value for a text field.
    const fn text_value_mut(self, form: &mut Form) -> Option<&mut String> {
        match self {
            Self::Name => Some(&mut form.name),
            Self::Command => Some(&mut form.command),
            Self::Args => Some(&mut form.args),
            Self::Url => Some(&mut form.url),
            Self::BearerToken => Some(&mut form.bearer_token),
            Self::ServerType | Self::AuthMode | Self::Scope => None,
        }
    }
}

// ── Enums ───────────────────────────────────────────────────────────────────
//
// `ServerType`, `AuthMode`, and `Scope` live in [`super::form_enums`] —
// re-imported at the top of this file for convenience.
