//! Form selector enums for the MCP setup overlay.
//!
//! Extracted from [`setup`](super::setup) for file-size hygiene. These
//! self-contained enums represent the three selector fields in the add-server
//! form: server type, authentication mode, and config scope.

// ── Server type ─────────────────────────────────────────────────────────────

/// Server type for the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerType {
    /// Local stdio server (command + args).
    Stdio,
    /// Remote HTTP/SSE server (url).
    Http,
}

impl ServerType {
    /// Cycle to the next variant (for selector toggle).
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Stdio => Self::Http,
            Self::Http => Self::Stdio,
        }
    }

    /// Cycle to the previous variant.
    #[must_use]
    pub const fn prev(self) -> Self {
        // Two-variant enum: prev == next.
        self.next()
    }

    /// Display label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }
}

// ── Authentication mode ─────────────────────────────────────────────────────

/// Authentication mode for the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// No authentication.
    None,
    /// Static bearer token.
    Bearer,
    /// OAuth 2.1 + PKCE browser flow.
    OAuth,
}

impl AuthMode {
    /// Cycle to the next variant.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::None => Self::Bearer,
            Self::Bearer => Self::OAuth,
            Self::OAuth => Self::None,
        }
    }

    /// Cycle to the previous variant.
    #[must_use]
    pub const fn prev(self) -> Self {
        match self {
            Self::None => Self::OAuth,
            Self::Bearer => Self::None,
            Self::OAuth => Self::Bearer,
        }
    }

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

// ── Config scope ────────────────────────────────────────────────────────────

/// Config scope for the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// User-wide: `~/.context-pilot/mcp.json`.
    Global,
    /// Per-project: `.context-pilot/shared/mcp.json`.
    Project,
}

impl Scope {
    /// Cycle to the next variant.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Global => Self::Project,
            Self::Project => Self::Global,
        }
    }

    /// Cycle to the previous variant.
    #[must_use]
    pub const fn prev(self) -> Self {
        // Two-variant enum: prev == next.
        self.next()
    }

    /// Display label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Global => "global (~/.context-pilot/mcp.json)",
            Self::Project => "project (.context-pilot/shared/mcp.json)",
        }
    }
}
