// =============================================================================
// API & MODELS
// =============================================================================

/// Model for TL;DR summarization
pub const MODEL_TLDR: &str = "claude-opus-4-5";

/// Maximum tokens for main response
pub const MAX_RESPONSE_TOKENS: u32 = 4096;

/// Maximum tokens for TL;DR summarization
pub const MAX_TLDR_TOKENS: u32 = 100;

/// Anthropic API endpoint
pub const API_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// Anthropic API version
pub const API_VERSION: &str = "2023-06-01";

// =============================================================================
// CONTEXT & TOKEN MANAGEMENT
// =============================================================================

/// Average characters per token for token estimation
pub const CHARS_PER_TOKEN: f32 = 4.0;

// =============================================================================
// PANEL CACHE DEPRECATION
// =============================================================================

/// Deprecation timer for glob panels (milliseconds)
pub const GLOB_DEPRECATION_MS: u64 = 30_000; // 30 seconds

/// Deprecation timer for grep panels (milliseconds)
pub const GREP_DEPRECATION_MS: u64 = 30_000; // 30 seconds

/// Deprecation timer for tmux panels (milliseconds)
pub const TMUX_DEPRECATION_MS: u64 = 1_000; // 1 second (check hash of last 2 lines)

/// Refresh interval for git status (milliseconds)
pub const GIT_STATUS_REFRESH_MS: u64 = 5_000; // 5 seconds

// =============================================================================
// SCROLLING
// =============================================================================

/// Scroll amount for Ctrl+Arrow keys
pub const SCROLL_ARROW_AMOUNT: f32 = 3.0;

/// Scroll amount for PageUp/PageDown
pub const SCROLL_PAGE_AMOUNT: f32 = 10.0;

/// Scroll acceleration increment per scroll event
pub const SCROLL_ACCEL_INCREMENT: f32 = 0.3;

/// Maximum scroll acceleration multiplier
pub const SCROLL_ACCEL_MAX: f32 = 2.5;

// =============================================================================
// TYPEWRITER EFFECT
// =============================================================================

/// Size of moving average for chunk timing
pub const TYPEWRITER_MOVING_AVG_SIZE: usize = 10;

/// Minimum character delay in milliseconds
pub const TYPEWRITER_MIN_DELAY_MS: f64 = 5.0;

/// Maximum character delay in milliseconds
pub const TYPEWRITER_MAX_DELAY_MS: f64 = 50.0;

/// Default character delay in milliseconds
pub const TYPEWRITER_DEFAULT_DELAY_MS: f64 = 15.0;

// =============================================================================
// UI LAYOUT
// =============================================================================

/// Width of the sidebar in characters
pub const SIDEBAR_WIDTH: u16 = 36;

/// Height of the status bar
pub const STATUS_BAR_HEIGHT: u16 = 1;

/// Height of the help hints section in sidebar
pub const SIDEBAR_HELP_HEIGHT: u16 = 8;

// =============================================================================
// EVENT LOOP
// =============================================================================

/// Poll interval for events in milliseconds
pub const EVENT_POLL_MS: u64 = 8;

/// Minimum time between renders (ms) - caps at ~28fps
pub const RENDER_THROTTLE_MS: u64 = 36;

/// Interval for CPU/RAM stats refresh in perf overlay (ms)
pub const PERF_STATS_REFRESH_MS: u64 = 500;

/// Delay after tmux send-keys in milliseconds (allows command output to appear)
pub const TMUX_SEND_DELAY_MS: u64 = 2000;

/// Fixed sleep duration in seconds for the sleep tool
pub const SLEEP_DURATION_SECS: u64 = 1;

/// Maximum number of retries for API errors
pub const MAX_API_RETRIES: u32 = 3;

// =============================================================================
// PERSISTENCE
// =============================================================================

/// Directory for storing state and messages
pub const STORE_DIR: &str = "./.context-pilot";

/// State file name
pub const STATE_FILE: &str = "state.json";

/// Messages subdirectory
pub const MESSAGES_DIR: &str = "messages";

// =============================================================================
// TMUX
// =============================================================================

/// Background session name for tmux operations
pub const TMUX_BG_SESSION: &str = "context-pilot-bg";

// =============================================================================
// THEME COLORS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

pub mod theme {
    use ratatui::style::Color;
    use crate::config::active_theme;

    fn rgb(c: [u8; 3]) -> Color {
        Color::Rgb(c[0], c[1], c[2])
    }

    // Primary brand colors
    pub fn accent() -> Color { rgb(active_theme().colors.accent) }
    pub fn accent_dim() -> Color { rgb(active_theme().colors.accent_dim) }
    pub fn success() -> Color { rgb(active_theme().colors.success) }
    pub fn warning() -> Color { rgb(active_theme().colors.warning) }
    pub fn error() -> Color { rgb(active_theme().colors.error) }

    // Text colors
    pub fn text() -> Color { rgb(active_theme().colors.text) }
    pub fn text_secondary() -> Color { rgb(active_theme().colors.text_secondary) }
    pub fn text_muted() -> Color { rgb(active_theme().colors.text_muted) }

    // Background colors
    pub fn bg_base() -> Color { rgb(active_theme().colors.bg_base) }
    pub fn bg_surface() -> Color { rgb(active_theme().colors.bg_surface) }
    pub fn bg_elevated() -> Color { rgb(active_theme().colors.bg_elevated) }

    // Border colors
    pub fn border() -> Color { rgb(active_theme().colors.border) }
    pub fn border_muted() -> Color { rgb(active_theme().colors.border_muted) }

    // Role-specific colors
    pub fn user() -> Color { rgb(active_theme().colors.user) }
    pub fn assistant() -> Color { rgb(active_theme().colors.assistant) }
}

// =============================================================================
// UI CHARACTERS
// =============================================================================

pub mod chars {
    pub const HORIZONTAL: &str = "─";
    pub const BLOCK_FULL: &str = "█";
    pub const BLOCK_LIGHT: &str = "░";
    pub const DOT: &str = "●";
    pub const ARROW_RIGHT: &str = "▸";
}

// =============================================================================
// ICONS / EMOJIS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

pub mod icons {
    use crate::config::active_theme;

    // Message types - accessor functions for active theme
    pub fn msg_user() -> &'static str { &active_theme().messages.user }
    pub fn msg_assistant() -> &'static str { &active_theme().messages.assistant }
    pub fn msg_tool_call() -> &'static str { &active_theme().messages.tool_call }
    pub fn msg_tool_result() -> &'static str { &active_theme().messages.tool_result }
    pub fn msg_error() -> &'static str { &active_theme().messages.error }

    // Context panel types
    pub fn ctx_system() -> &'static str { &active_theme().context.system }
    pub fn ctx_conversation() -> &'static str { &active_theme().context.conversation }
    pub fn ctx_tree() -> &'static str { &active_theme().context.tree }
    pub fn ctx_todo() -> &'static str { &active_theme().context.todo }
    pub fn ctx_memory() -> &'static str { &active_theme().context.memory }
    pub fn ctx_overview() -> &'static str { &active_theme().context.overview }
    pub fn ctx_file() -> &'static str { &active_theme().context.file }
    pub fn ctx_glob() -> &'static str { &active_theme().context.glob }
    pub fn ctx_grep() -> &'static str { &active_theme().context.grep }
    pub fn ctx_tmux() -> &'static str { &active_theme().context.tmux }
    pub fn ctx_git() -> &'static str { &active_theme().context.git }
    pub fn ctx_scratchpad() -> &'static str { &active_theme().context.scratchpad }

    // Message status
    pub fn status_full() -> &'static str { &active_theme().status.full }
    pub fn status_summarized() -> &'static str { &active_theme().status.summarized }
    pub fn status_deleted() -> &'static str { &active_theme().status.deleted }

    // Todo status
    pub fn todo_pending() -> &'static str { &active_theme().todo.pending }
    pub fn todo_in_progress() -> &'static str { &active_theme().todo.in_progress }
    pub fn todo_done() -> &'static str { &active_theme().todo.done }
}

// =============================================================================
// TOOL CATEGORY DESCRIPTIONS (loaded from yamls/ui.yaml via config module)
// =============================================================================

pub mod tool_categories {
    use crate::config::UI;

    pub fn file_desc() -> &'static str { &UI.tool_categories.file }
    pub fn tree_desc() -> &'static str { &UI.tool_categories.tree }
    pub fn console_desc() -> &'static str { &UI.tool_categories.console }
    pub fn context_desc() -> &'static str { &UI.tool_categories.context }
    pub fn todo_desc() -> &'static str { &UI.tool_categories.todo }
    pub fn memory_desc() -> &'static str { &UI.tool_categories.memory }
    pub fn git_desc() -> &'static str { &UI.tool_categories.git }
    pub fn scratchpad_desc() -> &'static str { &UI.tool_categories.scratchpad }
}

// =============================================================================
// PROMPTS (loaded from yamls/prompts.yaml via config module)
// =============================================================================

pub mod prompts {
    use crate::config::PROMPTS;

    pub fn default_seed_id() -> &'static str { &PROMPTS.default_seed.id }
    pub fn default_seed_name() -> &'static str { &PROMPTS.default_seed.name }
    pub fn default_seed_desc() -> &'static str { &PROMPTS.default_seed.description }
    pub fn default_seed_content() -> &'static str { &PROMPTS.default_seed.content }
    pub fn main_system() -> &'static str { &PROMPTS.default_seed.content }
    pub fn tldr_prompt() -> &'static str { &PROMPTS.tldr_prompt }
    pub fn tldr_min_tokens() -> usize { PROMPTS.tldr_min_tokens }
    pub fn panel_header() -> &'static str { &PROMPTS.panel.header }
    pub fn panel_timestamp() -> &'static str { &PROMPTS.panel.timestamp }
    pub fn panel_timestamp_unknown() -> &'static str { &PROMPTS.panel.timestamp_unknown }
    pub fn panel_footer() -> &'static str { &PROMPTS.panel.footer }
    pub fn panel_footer_msg_line() -> &'static str { &PROMPTS.panel.footer_msg_line }
    pub fn panel_footer_msg_header() -> &'static str { &PROMPTS.panel.footer_msg_header }
    pub fn panel_footer_ack() -> &'static str { &PROMPTS.panel.footer_ack }
}
