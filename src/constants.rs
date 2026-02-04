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
// THEME COLORS
// =============================================================================

pub mod theme {
    use ratatui::style::Color;

    // Primary brand colors
    pub const ACCENT: Color = Color::Rgb(218, 118, 89);        // #DA7659 - warm orange
    pub const ACCENT_DIM: Color = Color::Rgb(178, 98, 69);     // Dimmed warm orange
    pub const SUCCESS: Color = Color::Rgb(134, 188, 111);      // Soft green
    pub const WARNING: Color = Color::Rgb(229, 192, 123);      // Warm amber
    pub const ERROR: Color = Color::Rgb(200, 80, 80);          // Soft red for errors/deletions

    // Text colors
    pub const TEXT: Color = Color::Rgb(240, 240, 240);         // #f0f0f0 - primary text
    pub const TEXT_SECONDARY: Color = Color::Rgb(180, 180, 180); // Secondary text
    pub const TEXT_MUTED: Color = Color::Rgb(144, 144, 144);   // #909090 - muted text

    // Background colors
    pub const BG_BASE: Color = Color::Rgb(34, 34, 32);         // #222220 - darkest background
    pub const BG_SURFACE: Color = Color::Rgb(51, 51, 49);      // #333331 - content panels
    pub const BG_ELEVATED: Color = Color::Rgb(66, 66, 64);     // Elevated elements

    // Border colors
    pub const BORDER: Color = Color::Rgb(66, 66, 64);          // Subtle border
    pub const BORDER_MUTED: Color = Color::Rgb(50, 50, 48);    // Very subtle separator

    // Role-specific colors
    pub const USER: Color = Color::Rgb(218, 118, 89);          // Warm orange for user
    pub const ASSISTANT: Color = Color::Rgb(144, 144, 144);    // Muted for assistant
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
// ICONS / EMOJIS (loaded from yamls/icons.yaml via config module)
// =============================================================================

pub mod icons {
    use crate::config::ICONS;

    // Message types - accessor functions for lazy_static values
    pub fn msg_user() -> &'static str { &ICONS.messages.user }
    pub fn msg_assistant() -> &'static str { &ICONS.messages.assistant }
    pub fn msg_tool_call() -> &'static str { &ICONS.messages.tool_call }
    pub fn msg_tool_result() -> &'static str { &ICONS.messages.tool_result }
    pub fn msg_error() -> &'static str { &ICONS.messages.error }

    // Context panel types
    pub fn ctx_system() -> &'static str { &ICONS.context.system }
    pub fn ctx_conversation() -> &'static str { &ICONS.context.conversation }
    pub fn ctx_tree() -> &'static str { &ICONS.context.tree }
    pub fn ctx_todo() -> &'static str { &ICONS.context.todo }
    pub fn ctx_memory() -> &'static str { &ICONS.context.memory }
    pub fn ctx_overview() -> &'static str { &ICONS.context.overview }
    pub fn ctx_file() -> &'static str { &ICONS.context.file }
    pub fn ctx_glob() -> &'static str { &ICONS.context.glob }
    pub fn ctx_grep() -> &'static str { &ICONS.context.grep }
    pub fn ctx_tmux() -> &'static str { &ICONS.context.tmux }
    pub fn ctx_git() -> &'static str { &ICONS.context.git }
    pub fn ctx_scratchpad() -> &'static str { &ICONS.context.scratchpad }

    // Message status
    pub fn status_full() -> &'static str { &ICONS.status.full }
    pub fn status_summarized() -> &'static str { &ICONS.status.summarized }
    pub fn status_deleted() -> &'static str { &ICONS.status.deleted }

    // Todo status
    pub fn todo_pending() -> &'static str { &ICONS.todo.pending }
    pub fn todo_in_progress() -> &'static str { &ICONS.todo.in_progress }
    pub fn todo_done() -> &'static str { &ICONS.todo.done }
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
