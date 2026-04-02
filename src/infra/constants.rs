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

/// Theme color accessors loaded from the active YAML theme.
pub(crate) mod theme {
    use crate::infra::config::active_theme;
    use ratatui::style::Color;

    /// Convert an RGB array to a ratatui `Color`.
    const fn rgb(c: [u8; 3]) -> Color {
        Color::Rgb(c[0], c[1], c[2])
    }

    // Primary brand colors

    /// Accent color from the active theme.
    pub(crate) fn accent() -> Color {
        rgb(active_theme().colors.accent)
    }
    /// Dimmed accent color from the active theme.
    pub(crate) fn accent_dim() -> Color {
        rgb(active_theme().colors.accent_dim)
    }
    /// Success color from the active theme.
    pub(crate) fn success() -> Color {
        rgb(active_theme().colors.success)
    }
    /// Warning color from the active theme.
    pub(crate) fn warning() -> Color {
        rgb(active_theme().colors.warning)
    }
    /// Error color from the active theme.
    pub(crate) fn error() -> Color {
        rgb(active_theme().colors.error)
    }

    // Text colors

    /// Primary text color from the active theme.
    pub(crate) fn text() -> Color {
        rgb(active_theme().colors.text)
    }
    /// Secondary text color from the active theme.
    pub(crate) fn text_secondary() -> Color {
        rgb(active_theme().colors.text_secondary)
    }
    /// Muted text color from the active theme.
    pub(crate) fn text_muted() -> Color {
        rgb(active_theme().colors.text_muted)
    }

    // Background colors

    /// Base background color from the active theme.
    pub(crate) fn bg_base() -> Color {
        rgb(active_theme().colors.bg_base)
    }
    /// Surface background color from the active theme.
    pub(crate) fn bg_surface() -> Color {
        rgb(active_theme().colors.bg_surface)
    }
    /// Elevated background color from the active theme.
    pub(crate) fn bg_elevated() -> Color {
        rgb(active_theme().colors.bg_elevated)
    }

    // Border colors

    /// Border color from the active theme.
    pub(crate) fn border() -> Color {
        rgb(active_theme().colors.border)
    }
    /// Muted border color from the active theme.
    pub(crate) fn border_muted() -> Color {
        rgb(active_theme().colors.border_muted)
    }

    // Role-specific colors

    /// Assistant role color from the active theme.
    pub(crate) fn assistant() -> Color {
        rgb(active_theme().colors.assistant)
    }
}

// =============================================================================
// UI CHARACTERS
// =============================================================================

/// Unicode box-drawing and symbol characters for TUI rendering.
pub(crate) mod chars {
    /// Horizontal line character (box-drawing).
    pub(crate) const HORIZONTAL: &str = "─";
    /// Full block character.
    pub(crate) const BLOCK_FULL: &str = "█";
    /// Light shade block character.
    pub(crate) const BLOCK_LIGHT: &str = "░";
    /// Right-pointing arrow character.
    pub(crate) const ARROW_RIGHT: &str = "▸";
    /// Up-pointing arrow character.
    pub(crate) const ARROW_UP: &str = "↑";
    /// Down-pointing arrow character.
    pub(crate) const ARROW_DOWN: &str = "↓";
    /// Cross / multiplication sign character.
    pub(crate) const CROSS: &str = "✗";
}

// =============================================================================
// ICONS / EMOJIS (loaded from active theme in yamls/themes.yaml)
// All icons are normalized to 2 display cells width for consistent alignment
// =============================================================================

/// Icon accessors loaded from the active YAML theme, normalized to 2 cells.
pub(crate) mod icons {
    use crate::infra::config::{active_theme, normalize_icon};

    /// Icon for user messages (normalized to 2 cells).
    pub(crate) fn msg_user() -> String {
        normalize_icon(&active_theme().messages.user)
    }
    /// Icon for assistant messages (normalized to 2 cells).
    pub(crate) fn msg_assistant() -> String {
        normalize_icon(&active_theme().messages.assistant)
    }
    /// Icon for tool call messages (normalized to 2 cells).
    pub(crate) fn msg_tool_call() -> String {
        normalize_icon(&active_theme().messages.tool_call)
    }
    /// Icon for tool result messages (normalized to 2 cells).
    pub(crate) fn msg_tool_result() -> String {
        normalize_icon(&active_theme().messages.tool_result)
    }
    /// Icon for error messages (normalized to 2 cells).
    pub(crate) fn msg_error() -> String {
        normalize_icon(&active_theme().messages.error)
    }

    /// Icon for full status indicator (normalized to 2 cells).
    pub(crate) fn status_full() -> String {
        normalize_icon(&active_theme().status.full)
    }
    /// Icon for deleted status indicator (normalized to 2 cells).
    pub(crate) fn status_deleted() -> String {
        normalize_icon(&active_theme().status.deleted)
    }
}

// =============================================================================
// PROMPTS (loaded from yamls/prompts.yaml via config module)
// =============================================================================

/// Library content accessors loaded from YAML configuration.
pub(crate) mod library {
    use crate::infra::config::LIBRARY;

    /// Returns the default agent content from the library configuration.
    pub(crate) fn default_agent_content() -> &'static str {
        let id = &LIBRARY.default_agent_id;
        LIBRARY.agents.iter().find(|a| a.id == *id).map_or("", |a| a.content.as_str())
    }
}

/// Prompt template accessors loaded from YAML configuration.
pub(crate) mod prompts {
    use crate::infra::config::PROMPTS;

    /// Prompt template for panel headers.
    pub(crate) fn panel_header() -> &'static str {
        &PROMPTS.panel.header
    }
    /// Prompt template for panel timestamps.
    pub(crate) fn panel_timestamp() -> &'static str {
        &PROMPTS.panel.timestamp
    }
    /// Prompt template for unknown panel timestamps.
    pub(crate) fn panel_timestamp_unknown() -> &'static str {
        &PROMPTS.panel.timestamp_unknown
    }
    /// Prompt template for panel footers.
    pub(crate) fn panel_footer() -> &'static str {
        &PROMPTS.panel.footer
    }
    /// Prompt template for panel footer acknowledgment.
    pub(crate) fn panel_footer_ack() -> &'static str {
        &PROMPTS.panel.footer_ack
    }
}
