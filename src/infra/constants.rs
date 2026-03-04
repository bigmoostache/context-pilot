// =============================================================================
// API & MODELS
// =============================================================================

/// Anthropic API endpoint
pub(crate) const API_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// Anthropic API version
pub(crate) const API_VERSION: &str = "2023-06-01";

// =============================================================================
// CONTEXT & TOKEN MANAGEMENT
// =============================================================================

/// Minimum active messages in a chunk before it can be detached.
pub(crate) const DETACH_CHUNK_MIN_MESSAGES: usize = 25;

/// Minimum token count in a chunk before it can be detached.
pub(crate) const DETACH_CHUNK_MIN_TOKENS: usize = 5_000;

/// Minimum messages to keep in the live conversation after detachment.
pub(crate) const DETACH_KEEP_MIN_MESSAGES: usize = 10;

/// Minimum tokens to keep in the live conversation after detachment.
pub(crate) const DETACH_KEEP_MIN_TOKENS: usize = 1_000;

// =============================================================================
// SCROLLING
// =============================================================================

/// Scroll amount for Ctrl+Arrow keys
pub(crate) const SCROLL_ARROW_AMOUNT: f32 = 3.0;

/// Scroll amount for PageUp/PageDown
pub(crate) const SCROLL_PAGE_AMOUNT: f32 = 10.0;

/// Scroll acceleration increment per scroll event
pub(crate) const SCROLL_ACCEL_INCREMENT: f32 = 0.3;

/// Maximum scroll acceleration multiplier
pub(crate) const SCROLL_ACCEL_MAX: f32 = 2.5;

// =============================================================================
// TYPEWRITER EFFECT
// =============================================================================

/// Size of moving average for chunk timing
pub(crate) const TYPEWRITER_MOVING_AVG_SIZE: usize = 10;

/// Minimum character delay in milliseconds
pub(crate) const TYPEWRITER_MIN_DELAY_MS: f64 = 5.0;

/// Maximum character delay in milliseconds
pub(crate) const TYPEWRITER_MAX_DELAY_MS: f64 = 50.0;

/// Default character delay in milliseconds
pub(crate) const TYPEWRITER_DEFAULT_DELAY_MS: f64 = 15.0;

// =============================================================================
// UI LAYOUT
// =============================================================================

/// Height of the status bar
pub(crate) const STATUS_BAR_HEIGHT: u16 = 1;

/// Height of the help hints section in sidebar
pub(crate) const SIDEBAR_HELP_HEIGHT: u16 = 9;

// =============================================================================
// EVENT LOOP
// =============================================================================

/// Poll interval for events in milliseconds
pub(crate) const EVENT_POLL_MS: u64 = 8;

/// Minimum time between renders (ms) - caps at ~28fps
pub(crate) const RENDER_THROTTLE_MS: u64 = 36;

/// Interval for CPU/RAM stats refresh in perf overlay (ms)
pub(crate) const PERF_STATS_REFRESH_MS: u64 = 500;

/// Maximum number of retries for API errors
pub(crate) const MAX_API_RETRIES: u32 = 3;

// =============================================================================
// REVERIE (CONTEXT OPTIMIZER SUB-AGENT)
// =============================================================================

/// Maximum tool calls per reverie run before force-stopping
pub(crate) const REVERIE_TOOL_CAP: usize = 50;

// =============================================================================
// PERSISTENCE
// =============================================================================

/// Directory for storing state and messages
pub(crate) const STORE_DIR: &str = "./.context-pilot";

/// Messages subdirectory
pub(crate) const MESSAGES_DIR: &str = "messages";

/// Shared config file name (new multi-worker format)
pub(crate) const CONFIG_FILE: &str = "config.json";

/// Worker states subdirectory
pub(crate) const STATES_DIR: &str = "states";

/// Panel data subdirectory (for dynamic panels)
pub(crate) const PANELS_DIR: &str = "panels";

/// Default worker ID
pub(crate) const DEFAULT_WORKER_ID: &str = "main_worker";

// =============================================================================
// THEME COLORS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

pub(crate) mod theme {
    use crate::infra::config::active_theme;
    use ratatui::style::Color;

    const fn rgb(c: [u8; 3]) -> Color {
        Color::Rgb(c[0], c[1], c[2])
    }

    // Primary brand colors
    pub(crate) fn accent() -> Color {
        rgb(active_theme().colors.accent)
    }
    pub(crate) fn accent_dim() -> Color {
        rgb(active_theme().colors.accent_dim)
    }
    pub(crate) fn success() -> Color {
        rgb(active_theme().colors.success)
    }
    pub(crate) fn warning() -> Color {
        rgb(active_theme().colors.warning)
    }
    pub(crate) fn error() -> Color {
        rgb(active_theme().colors.error)
    }

    // Text colors
    pub(crate) fn text() -> Color {
        rgb(active_theme().colors.text)
    }
    pub(crate) fn text_secondary() -> Color {
        rgb(active_theme().colors.text_secondary)
    }
    pub(crate) fn text_muted() -> Color {
        rgb(active_theme().colors.text_muted)
    }

    // Background colors
    pub(crate) fn bg_base() -> Color {
        rgb(active_theme().colors.bg_base)
    }
    pub(crate) fn bg_surface() -> Color {
        rgb(active_theme().colors.bg_surface)
    }
    pub(crate) fn bg_elevated() -> Color {
        rgb(active_theme().colors.bg_elevated)
    }

    // Border colors
    pub(crate) fn border() -> Color {
        rgb(active_theme().colors.border)
    }
    pub(crate) fn border_muted() -> Color {
        rgb(active_theme().colors.border_muted)
    }

    // Role-specific colors
    pub(crate) fn user() -> Color {
        rgb(active_theme().colors.user)
    }
    pub(crate) fn assistant() -> Color {
        rgb(active_theme().colors.assistant)
    }
}

// =============================================================================
// UI CHARACTERS
// =============================================================================

pub(crate) mod chars {
    pub(crate) const HORIZONTAL: &str = "─";
    pub(crate) const BLOCK_FULL: &str = "█";
    pub(crate) const BLOCK_LIGHT: &str = "░";
    pub(crate) const ARROW_RIGHT: &str = "▸";
    pub(crate) const ARROW_UP: &str = "↑";
    pub(crate) const ARROW_DOWN: &str = "↓";
    pub(crate) const CROSS: &str = "✗";
}

// =============================================================================
// ICONS / EMOJIS (loaded from active theme in yamls/themes.yaml)
// All icons are normalized to 2 display cells width for consistent alignment
// =============================================================================

pub(crate) mod icons {
    use crate::infra::config::{active_theme, normalize_icon};

    // Message types - accessor functions for active theme (normalized to 2 cells)
    pub(crate) fn msg_user() -> String {
        normalize_icon(&active_theme().messages.user)
    }
    pub(crate) fn msg_assistant() -> String {
        normalize_icon(&active_theme().messages.assistant)
    }
    pub(crate) fn msg_tool_call() -> String {
        normalize_icon(&active_theme().messages.tool_call)
    }
    pub(crate) fn msg_tool_result() -> String {
        normalize_icon(&active_theme().messages.tool_result)
    }
    pub(crate) fn msg_error() -> String {
        normalize_icon(&active_theme().messages.error)
    }

    // Message status (normalized to 2 cells)
    pub(crate) fn status_full() -> String {
        normalize_icon(&active_theme().status.full)
    }
    pub(crate) fn status_deleted() -> String {
        normalize_icon(&active_theme().status.deleted)
    }
}

// =============================================================================
// PROMPTS (loaded from yamls/prompts.yaml via config module)
// =============================================================================

pub(crate) mod library {
    use crate::infra::config::LIBRARY;

    pub(crate) fn default_agent_content() -> &'static str {
        let id = &LIBRARY.default_agent_id;
        LIBRARY.agents.iter().find(|a| a.id == *id).map_or("", |a| a.content.as_str())
    }
}

pub(crate) mod prompts {
    use crate::infra::config::PROMPTS;

    pub(crate) fn panel_header() -> &'static str {
        &PROMPTS.panel.header
    }
    pub(crate) fn panel_timestamp() -> &'static str {
        &PROMPTS.panel.timestamp
    }
    pub(crate) fn panel_timestamp_unknown() -> &'static str {
        &PROMPTS.panel.timestamp_unknown
    }
    pub(crate) fn panel_footer() -> &'static str {
        &PROMPTS.panel.footer
    }
    pub(crate) fn panel_footer_msg_line() -> &'static str {
        &PROMPTS.panel.footer_msg_line
    }
    pub(crate) fn panel_footer_msg_header() -> &'static str {
        &PROMPTS.panel.footer_msg_header
    }
    pub(crate) fn panel_footer_ack() -> &'static str {
        &PROMPTS.panel.footer_ack
    }
}
