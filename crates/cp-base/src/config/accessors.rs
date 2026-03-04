//! Thin accessor modules for compile-time embedded configuration.
//!
//! Each sub-module provides zero-cost getters backed by static `LazyLock` singletons.
//! Grouped here to keep `config/mod.rs` focused on type definitions and loading.

// =============================================================================
// THEME COLORS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

/// Theme color accessors — each returns a `ratatui::style::Color::Rgb`
/// from the currently active theme. Zero-cost after first call (atomic pointer).
pub mod theme {
    use crate::config::active_theme;
    use ratatui::style::Color;

    /// Convert an `[r, g, b]` triple to a ratatui RGB color.
    fn rgb(c: [u8; 3]) -> Color {
        Color::Rgb(c[0], c[1], c[2])
    }

    /// Primary accent color.
    pub fn accent() -> Color {
        rgb(active_theme().colors.accent)
    }
    /// Dimmed accent for inactive highlights.
    pub fn accent_dim() -> Color {
        rgb(active_theme().colors.accent_dim)
    }
    /// Success indicator color.
    pub fn success() -> Color {
        rgb(active_theme().colors.success)
    }
    /// Warning indicator color.
    pub fn warning() -> Color {
        rgb(active_theme().colors.warning)
    }
    /// Error indicator color.
    pub fn error() -> Color {
        rgb(active_theme().colors.error)
    }
    /// Primary text color.
    pub fn text() -> Color {
        rgb(active_theme().colors.text)
    }
    /// Secondary text color (labels, metadata).
    pub fn text_secondary() -> Color {
        rgb(active_theme().colors.text_secondary)
    }
    /// Muted text color (hints, disabled).
    pub fn text_muted() -> Color {
        rgb(active_theme().colors.text_muted)
    }
    /// Base background color.
    pub fn bg_base() -> Color {
        rgb(active_theme().colors.bg_base)
    }
    /// Elevated surface background (panels).
    pub fn bg_surface() -> Color {
        rgb(active_theme().colors.bg_surface)
    }
    /// Highest-elevation background (popups, overlays).
    pub fn bg_elevated() -> Color {
        rgb(active_theme().colors.bg_elevated)
    }
    /// Primary border color.
    pub fn border() -> Color {
        rgb(active_theme().colors.border)
    }
    /// Subtle border color (dividers).
    pub fn border_muted() -> Color {
        rgb(active_theme().colors.border_muted)
    }
    /// User message accent color.
    pub fn user() -> Color {
        rgb(active_theme().colors.user)
    }
    /// Assistant message accent color.
    pub fn assistant() -> Color {
        rgb(active_theme().colors.assistant)
    }
}

// =============================================================================
// UI CHARACTERS
// =============================================================================

/// Unicode box-drawing and indicator characters for TUI rendering.
pub mod chars {
    /// Horizontal line segment (─).
    pub const HORIZONTAL: &str = "─";
    /// Full-width block (█).
    pub const BLOCK_FULL: &str = "█";
    /// Light shade block (░).
    pub const BLOCK_LIGHT: &str = "░";
    /// Filled circle (●).
    pub const DOT: &str = "●";
    /// Right-pointing triangle (▸).
    pub const ARROW_RIGHT: &str = "▸";
    /// Up arrow (↑).
    pub const ARROW_UP: &str = "↑";
    /// Down arrow (↓).
    pub const ARROW_DOWN: &str = "↓";
    /// Cross mark (✗).
    pub const CROSS: &str = "✗";
}

// =============================================================================
// ICONS / EMOJIS (loaded from active theme in yamls/themes.yaml)
// All icons are normalized to 2 display cells width for consistent alignment
// =============================================================================

pub mod icons {
    //! Message and context icons from the active theme.
    use crate::config::{active_theme, normalize_icon};

    /// User message icon (e.g., "⚔ ").
    pub fn msg_user() -> String {
        normalize_icon(&active_theme().messages.user)
    }
    /// Assistant message icon (e.g., "🐉 ").
    pub fn msg_assistant() -> String {
        normalize_icon(&active_theme().messages.assistant)
    }
    /// Tool-call message icon.
    pub fn msg_tool_call() -> String {
        normalize_icon(&active_theme().messages.tool_call)
    }
    /// Tool-result message icon.
    pub fn msg_tool_result() -> String {
        normalize_icon(&active_theme().messages.tool_result)
    }
    /// Error message icon.
    pub fn msg_error() -> String {
        normalize_icon(&active_theme().messages.error)
    }
    /// Status icon for messages included in full.
    pub fn status_full() -> String {
        normalize_icon(&active_theme().status.full)
    }
    /// Status icon for deleted/detached messages.
    pub fn status_deleted() -> String {
        normalize_icon(&active_theme().status.deleted)
    }
    /// Todo icon for pending items.
    pub fn todo_pending() -> String {
        normalize_icon(&active_theme().todo.pending)
    }
    /// Todo icon for in-progress items.
    pub fn todo_in_progress() -> String {
        normalize_icon(&active_theme().todo.in_progress)
    }
    /// Todo icon for completed items.
    pub fn todo_done() -> String {
        normalize_icon(&active_theme().todo.done)
    }
}

// =============================================================================
// PROMPTS (loaded from yamls/prompts.yaml via config module)
// =============================================================================

pub mod library {
    //! Accessors for the seed prompt library (agents, skills, commands).
    use crate::config::LIBRARY;

    /// Default agent ID (used when none is selected).
    pub fn default_agent_id() -> &'static str {
        &LIBRARY.default_agent_id
    }
    /// Content body of the default agent.
    pub fn default_agent_content() -> &'static str {
        let id = &LIBRARY.default_agent_id;
        LIBRARY.agents.iter().find(|a| a.id == *id).map_or("", |a| a.content.as_str())
    }
    /// All built-in agent definitions.
    pub fn agents() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.agents
    }
    /// All built-in skill definitions.
    pub fn skills() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.skills
    }
    /// All built-in command definitions.
    pub fn commands() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.commands
    }
}

pub mod prompts {
    //! Accessors for prompt templates (panel header/footer formatting).
    use crate::config::PROMPTS;

    /// Panel opening header template (`{id}`, `{type}`, `{name}` placeholders).
    pub fn panel_header() -> &'static str {
        &PROMPTS.panel.header
    }
    /// Panel timestamp template (`{timestamp}` placeholder).
    pub fn panel_timestamp() -> &'static str {
        &PROMPTS.panel.timestamp
    }
    /// Fallback when panel has no known timestamp.
    pub fn panel_timestamp_unknown() -> &'static str {
        &PROMPTS.panel.timestamp_unknown
    }
    /// Panel closing footer template.
    pub fn panel_footer() -> &'static str {
        &PROMPTS.panel.footer
    }
    /// Format for a message line inside footer.
    pub fn panel_footer_msg_line() -> &'static str {
        &PROMPTS.panel.footer_msg_line
    }
    /// Header for recent-messages section in footer.
    pub fn panel_footer_msg_header() -> &'static str {
        &PROMPTS.panel.footer_msg_header
    }
    /// Assistant ack injected after footer.
    pub fn panel_footer_ack() -> &'static str {
        &PROMPTS.panel.footer_ack
    }
}
