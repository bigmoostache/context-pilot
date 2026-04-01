//! Thin accessor modules for compile-time embedded configuration.
//!
//! Each sub-module provides zero-cost getters backed by static `LazyLock` singletons.
//! Grouped here to keep `config/mod.rs` focused on type definitions and loading.

use std::sync::atomic::{AtomicU8, Ordering};

use super::{DEFAULT_THEME, THEME_ORDER, THEMES, Theme, invariant_panic};

// =============================================================================
// ACTIVE THEME (Global State — index-based lookup, fully safe)
// =============================================================================

/// Index into [`THEME_ORDER`] for the active theme.
/// `u8::MAX` = not yet set (uses [`DEFAULT_THEME`] on first access).
static CACHED_THEME_IDX: AtomicU8 = AtomicU8::new(u8::MAX);

/// Resolve a theme-order index to its theme reference.
fn theme_by_index(idx: u8) -> Option<&'static Theme> {
    let id = THEME_ORDER.get(idx as usize)?;
    THEMES.themes.get(*id)
}

/// Find the index of `theme_id` in [`THEME_ORDER`], or `None` if absent.
fn theme_index(theme_id: &str) -> Option<u8> {
    let i = THEME_ORDER.iter().position(|&id| id == theme_id)?;
    u8::try_from(i).ok()
}

/// Set the active theme ID (call when state is loaded or theme changes).
pub fn set_active_theme(theme_id: &str) {
    if let Some(idx) = theme_index(theme_id) {
        CACHED_THEME_IDX.store(idx, Ordering::Release);
    }
}

/// Get the currently active theme (atomic load + `HashMap` lookup — no unsafe).
///
/// Falls back to the default theme, then to any available theme.
///
/// # Panics
///
/// Panics if the themes map contains zero entries.
pub fn active_theme() -> &'static Theme {
    let idx = CACHED_THEME_IDX.load(Ordering::Acquire);
    let resolve = |theme: Option<&'static Theme>| -> &'static Theme {
        theme.unwrap_or_else(|| invariant_panic("themes.yaml has no themes"))
    };
    if idx == u8::MAX {
        // First call before set_active_theme — initialize from default
        let default_idx = theme_index(DEFAULT_THEME);
        if let Some(di) = default_idx {
            CACHED_THEME_IDX.store(di, Ordering::Release);
        }
        resolve(default_idx.and_then(theme_by_index).or_else(|| THEMES.themes.values().next()))
    } else {
        resolve(theme_by_index(idx).or_else(|| THEMES.themes.values().next()))
    }
}

// =============================================================================
// THEME COLORS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

/// Theme color accessors — each returns a `ratatui::style::Color::Rgb`
/// from the currently active theme. Zero-cost after first call (atomic pointer).
pub mod theme {
    use super::active_theme;
    use ratatui::style::Color;

    /// Convert an `[r, g, b]` triple to a ratatui RGB color.
    const fn rgb(c: [u8; 3]) -> Color {
        Color::Rgb(c[0], c[1], c[2])
    }

    /// Primary accent color.
    #[must_use]
    pub fn accent() -> Color {
        rgb(active_theme().colors.accent)
    }
    /// Dimmed accent for inactive highlights.
    #[must_use]
    pub fn accent_dim() -> Color {
        rgb(active_theme().colors.accent_dim)
    }
    /// Success indicator color.
    #[must_use]
    pub fn success() -> Color {
        rgb(active_theme().colors.success)
    }
    /// Warning indicator color.
    #[must_use]
    pub fn warning() -> Color {
        rgb(active_theme().colors.warning)
    }
    /// Error indicator color.
    #[must_use]
    pub fn error() -> Color {
        rgb(active_theme().colors.error)
    }
    /// Primary text color.
    #[must_use]
    pub fn text() -> Color {
        rgb(active_theme().colors.text)
    }
    /// Secondary text color (labels, metadata).
    #[must_use]
    pub fn text_secondary() -> Color {
        rgb(active_theme().colors.text_secondary)
    }
    /// Muted text color (hints, disabled).
    #[must_use]
    pub fn text_muted() -> Color {
        rgb(active_theme().colors.text_muted)
    }
    /// Base background color.
    #[must_use]
    pub fn bg_base() -> Color {
        rgb(active_theme().colors.bg_base)
    }
    /// Elevated surface background (panels).
    #[must_use]
    pub fn bg_surface() -> Color {
        rgb(active_theme().colors.bg_surface)
    }
    /// Highest-elevation background (popups, overlays).
    #[must_use]
    pub fn bg_elevated() -> Color {
        rgb(active_theme().colors.bg_elevated)
    }
    /// Primary border color.
    #[must_use]
    pub fn border() -> Color {
        rgb(active_theme().colors.border)
    }
    /// Subtle border color (dividers).
    #[must_use]
    pub fn border_muted() -> Color {
        rgb(active_theme().colors.border_muted)
    }
    /// User message accent color.
    #[must_use]
    pub fn user() -> Color {
        rgb(active_theme().colors.user)
    }
    /// Assistant message accent color.
    #[must_use]
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
    use super::active_theme;
    use crate::config::normalize_icon;

    /// User message icon (e.g., "⚔ ").
    #[must_use]
    pub fn msg_user() -> String {
        normalize_icon(&active_theme().messages.user)
    }
    /// Assistant message icon (e.g., "🐉 ").
    #[must_use]
    pub fn msg_assistant() -> String {
        normalize_icon(&active_theme().messages.assistant)
    }
    /// Tool-call message icon.
    #[must_use]
    pub fn msg_tool_call() -> String {
        normalize_icon(&active_theme().messages.tool_call)
    }
    /// Tool-result message icon.
    #[must_use]
    pub fn msg_tool_result() -> String {
        normalize_icon(&active_theme().messages.tool_result)
    }
    /// Error message icon.
    #[must_use]
    pub fn msg_error() -> String {
        normalize_icon(&active_theme().messages.error)
    }
    /// Status icon for messages included in full.
    #[must_use]
    pub fn status_full() -> String {
        normalize_icon(&active_theme().status.full)
    }
    /// Status icon for deleted/detached messages.
    #[must_use]
    pub fn status_deleted() -> String {
        normalize_icon(&active_theme().status.deleted)
    }
    /// Todo icon for pending items.
    #[must_use]
    pub fn todo_pending() -> String {
        normalize_icon(&active_theme().todo.pending)
    }
    /// Todo icon for in-progress items.
    #[must_use]
    pub fn todo_in_progress() -> String {
        normalize_icon(&active_theme().todo.in_progress)
    }
    /// Todo icon for completed items.
    #[must_use]
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
    #[must_use]
    pub fn default_agent_id() -> &'static str {
        &LIBRARY.default_agent_id
    }
    /// Content body of the default agent.
    #[must_use]
    pub fn default_agent_content() -> &'static str {
        let id = &LIBRARY.default_agent_id;
        LIBRARY.agents.iter().find(|a| a.id == *id).map_or("", |a| a.content.as_str())
    }
    /// All built-in agent definitions.
    #[must_use]
    pub fn agents() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.agents
    }
    /// All built-in skill definitions.
    #[must_use]
    pub fn skills() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.skills
    }
    /// All built-in command definitions.
    #[must_use]
    pub fn commands() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.commands
    }
}

pub mod prompts {
    //! Accessors for prompt templates (panel header/footer formatting).
    use crate::config::PROMPTS;

    /// Panel opening header template (`{id}`, `{type}`, `{name}` placeholders).
    #[must_use]
    pub fn panel_header() -> &'static str {
        &PROMPTS.panel.header
    }
    /// Panel timestamp template (`{timestamp}` placeholder).
    #[must_use]
    pub fn panel_timestamp() -> &'static str {
        &PROMPTS.panel.timestamp
    }
    /// Fallback when panel has no known timestamp.
    #[must_use]
    pub fn panel_timestamp_unknown() -> &'static str {
        &PROMPTS.panel.timestamp_unknown
    }
    /// Panel closing footer template.
    #[must_use]
    pub fn panel_footer() -> &'static str {
        &PROMPTS.panel.footer
    }
    /// Assistant ack injected after footer.
    #[must_use]
    pub fn panel_footer_ack() -> &'static str {
        &PROMPTS.panel.footer_ack
    }
}
