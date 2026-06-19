//! MCP setup overlay IR types (Ctrl+M or keybinding TBD).
//!
//! Models the overlay for configuring MCP servers: listing connected servers,
//! adding new ones (stdio / HTTP), choosing auth mode, and selecting scope
//! (global vs project). Built by the UI builder, rendered by the renderer.

use serde::Serialize;

use crate::Semantic;

/// MCP server setup overlay.
#[derive(Debug, Clone, Serialize)]
pub struct McpSetupOverlay {
    /// Title for the overlay window.
    pub title: String,
    /// Configured servers with their current status.
    pub servers: Vec<McpSetupServer>,
    /// Index of the currently highlighted server (for navigation).
    pub selected_index: usize,
    /// Current interaction mode.
    pub mode: McpMode,
    /// Add/edit form state (present when `mode` is `AddForm` or `EditForm`).
    pub form: Option<McpFormIR>,
    /// Transient error message to display (e.g. save failure).
    pub error: Option<String>,
    /// Transient success message (e.g. "Server added ✓").
    pub success: Option<String>,
    /// Footer help text.
    pub footer: String,
}

/// One server entry in the setup overlay list.
#[derive(Debug, Clone, Serialize)]
pub struct McpSetupServer {
    /// Server name (key in mcp.json).
    pub name: String,
    /// Type label: "stdio" or "http".
    pub server_type: String,
    /// Connection status display (e.g. "Connected (5 tools)", "Failed: timeout").
    pub status_label: String,
    /// Semantic colour for the status.
    pub status_semantic: Semantic,
    /// Auth mode display: "none", "bearer", "oauth", "auto".
    pub auth_label: String,
    /// Config scope: "global" or "project".
    pub scope: String,
    /// Number of tools exposed by this server.
    pub tool_count: usize,
    /// Whether this entry is currently selected/highlighted.
    pub selected: bool,
}

/// Interaction mode for the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum McpMode {
    /// Browsing the server list.
    List,
    /// Filling out the add-server form.
    AddForm,
    /// Confirming deletion of the selected server.
    ConfirmDelete,
    /// OAuth flow in progress (waiting for browser callback).
    OAuthPending,
}

/// Snapshot of the add/edit form for rendering.
#[derive(Debug, Clone, Serialize)]
pub struct McpFormIR {
    /// Form title (e.g. "Add Server", "Edit Server").
    pub title: String,
    /// Server name field.
    pub name: McpFormField,
    /// Server type selection: stdio or http.
    pub server_type: McpServerType,
    /// Command field (stdio only).
    pub command: McpFormField,
    /// Arguments field (stdio only, space-separated).
    pub args: McpFormField,
    /// URL field (http only).
    pub url: McpFormField,
    /// Bearer token field (http + bearer auth only).
    pub bearer_token: McpFormField,
    /// Selected authentication mode.
    pub auth_mode: McpAuthMode,
    /// Config scope (global / project).
    pub scope: McpScope,
    /// Index of the currently focused form element (for highlight).
    pub focused_field: usize,
    /// Total number of navigable fields.
    pub field_count: usize,
}

/// A single form text field.
#[derive(Debug, Clone, Serialize)]
pub struct McpFormField {
    /// Display label (e.g. "Server Name").
    pub label: String,
    /// Current text value.
    pub value: String,
    /// Placeholder text shown when empty.
    pub placeholder: String,
    /// Whether this field is currently focused.
    pub focused: bool,
    /// Whether this field is visible (some fields hide based on server type).
    pub visible: bool,
    /// Cursor position (character index) within the value. Only meaningful when focused.
    pub cursor_pos: usize,
}

/// Server type selection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum McpServerType {
    /// Local stdio server (command + args).
    Stdio,
    /// Remote HTTP/SSE server (url).
    Http,
}

impl McpServerType {
    /// Display label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }
}

/// Authentication mode selection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum McpAuthMode {
    /// No authentication.
    None,
    /// Static bearer token.
    Bearer,
    /// OAuth 2.1 + PKCE browser flow.
    OAuth,
}

impl McpAuthMode {
    /// Display label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bearer => "bearer",
            Self::OAuth => "oauth",
        }
    }
}

/// Config scope selection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum McpScope {
    /// User-wide: `~/.context-pilot/mcp.json`.
    Global,
    /// Per-project: `.context-pilot/shared/mcp.json`.
    Project,
}

impl McpScope {
    /// Display label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Global => "global (~/.context-pilot/mcp.json)",
            Self::Project => "project (.context-pilot/shared/mcp.json)",
        }
    }
}
