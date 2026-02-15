// =============================================================================
// API & MODELS
// =============================================================================

/// Model for TL;DR summarization
pub const MODEL_TLDR: &str = "claude-opus-4-5";

/// Maximum tokens for main response
pub const MAX_RESPONSE_TOKENS: u32 = 16384;

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
pub const CHARS_PER_TOKEN: f32 = 3.3;

/// Maximum token length for memory tl_dr field (enforced on create/update)
pub const MEMORY_TLDR_MAX_TOKENS: usize = 80;

/// Minimum active messages in a chunk before it can be detached.
pub const DETACH_CHUNK_MIN_MESSAGES: usize = 25;

/// Minimum token count in a chunk before it can be detached.
pub const DETACH_CHUNK_MIN_TOKENS: usize = 5_000;

/// Minimum messages to keep in the live conversation after detachment.
pub const DETACH_KEEP_MIN_MESSAGES: usize = 25;

/// Minimum tokens to keep in the live conversation after detachment.
pub const DETACH_KEEP_MIN_TOKENS: usize = 7_500;

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
pub const TMUX_SEND_DELAY_MS: u64 = 1000;

/// Fixed sleep duration in seconds for the sleep tool
pub const SLEEP_DURATION_SECS: u64 = 1;

/// Maximum number of retries for API errors
pub const MAX_API_RETRIES: u32 = 3;

// =============================================================================
// PERSISTENCE
// =============================================================================

/// Directory for storing state and messages
pub const STORE_DIR: &str = "./.context-pilot";

/// Messages subdirectory
pub const MESSAGES_DIR: &str = "messages";

/// Shared config file name (new multi-worker format)
pub const CONFIG_FILE: &str = "config.json";

/// Worker states subdirectory
pub const STATES_DIR: &str = "states";

/// Panel data subdirectory (for dynamic panels)
pub const PANELS_DIR: &str = "panels";

/// Default worker ID
pub const DEFAULT_WORKER_ID: &str = "main_worker";

/// Presets subdirectory
pub const PRESETS_DIR: &str = "presets";

/// Logs subdirectory (chunked JSON files, global across workers)
pub const LOGS_DIR: &str = "logs";

/// Number of log entries per chunk file
pub const LOGS_CHUNK_SIZE: usize = 1000;

// =============================================================================
// PANEL SIZE LIMITS
// =============================================================================

/// Hard cap: refuse to load any panel content larger than this (bytes)
pub const PANEL_MAX_LOAD_BYTES: usize = 5 * 1024 * 1024; // 5 MB

/// Tokens per page when paginating (also serves as the soft cap — panels exceeding this get paginated)
pub const PANEL_PAGE_TOKENS: usize = 25_000;

// =============================================================================
// TMUX
// =============================================================================

/// Background session name for tmux operations
pub const TMUX_BG_SESSION: &str = "context-pilot-bg";

/// Maximum size for command output cached in result panels (bytes)
pub const MAX_RESULT_CONTENT_BYTES: usize = 1_000_000; // 1 MB

/// Timeout for git commands (seconds)
pub const GIT_CMD_TIMEOUT_SECS: u64 = 30;

/// Timeout for gh commands (seconds)
pub const GH_CMD_TIMEOUT_SECS: u64 = 60;

// =============================================================================
// THEME COLORS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

pub mod theme {
    use crate::config::active_theme;
    use ratatui::style::Color;

    fn rgb(c: [u8; 3]) -> Color {
        Color::Rgb(c[0], c[1], c[2])
    }

    // Primary brand colors
    pub fn accent() -> Color {
        rgb(active_theme().colors.accent)
    }
    pub fn accent_dim() -> Color {
        rgb(active_theme().colors.accent_dim)
    }
    pub fn success() -> Color {
        rgb(active_theme().colors.success)
    }
    pub fn warning() -> Color {
        rgb(active_theme().colors.warning)
    }
    pub fn error() -> Color {
        rgb(active_theme().colors.error)
    }

    // Text colors
    pub fn text() -> Color {
        rgb(active_theme().colors.text)
    }
    pub fn text_secondary() -> Color {
        rgb(active_theme().colors.text_secondary)
    }
    pub fn text_muted() -> Color {
        rgb(active_theme().colors.text_muted)
    }

    // Background colors
    pub fn bg_base() -> Color {
        rgb(active_theme().colors.bg_base)
    }
    pub fn bg_surface() -> Color {
        rgb(active_theme().colors.bg_surface)
    }
    pub fn bg_elevated() -> Color {
        rgb(active_theme().colors.bg_elevated)
    }

    // Border colors
    pub fn border() -> Color {
        rgb(active_theme().colors.border)
    }
    pub fn border_muted() -> Color {
        rgb(active_theme().colors.border_muted)
    }

    // Role-specific colors
    pub fn user() -> Color {
        rgb(active_theme().colors.user)
    }
    pub fn assistant() -> Color {
        rgb(active_theme().colors.assistant)
    }
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
    pub const ARROW_UP: &str = "↑";
    pub const ARROW_DOWN: &str = "↓";
    pub const CROSS: &str = "✗";
}

// =============================================================================
// ICONS / EMOJIS (loaded from active theme in yamls/themes.yaml)
// All icons are normalized to 2 display cells width for consistent alignment
// =============================================================================

pub mod icons {
    use crate::config::{active_theme, normalize_icon};

    // Message types - accessor functions for active theme (normalized to 2 cells)
    pub fn msg_user() -> String {
        normalize_icon(&active_theme().messages.user)
    }
    pub fn msg_assistant() -> String {
        normalize_icon(&active_theme().messages.assistant)
    }
    pub fn msg_tool_call() -> String {
        normalize_icon(&active_theme().messages.tool_call)
    }
    pub fn msg_tool_result() -> String {
        normalize_icon(&active_theme().messages.tool_result)
    }
    pub fn msg_error() -> String {
        normalize_icon(&active_theme().messages.error)
    }

    // Context panel types (normalized to 2 cells)
    pub fn ctx_system() -> String {
        normalize_icon(&active_theme().context.system)
    }
    pub fn ctx_conversation() -> String {
        normalize_icon(&active_theme().context.conversation)
    }
    pub fn ctx_tree() -> String {
        normalize_icon(&active_theme().context.tree)
    }
    pub fn ctx_todo() -> String {
        normalize_icon(&active_theme().context.todo)
    }
    pub fn ctx_memory() -> String {
        normalize_icon(&active_theme().context.memory)
    }
    pub fn ctx_overview() -> String {
        normalize_icon(&active_theme().context.overview)
    }
    pub fn ctx_file() -> String {
        normalize_icon(&active_theme().context.file)
    }
    pub fn ctx_glob() -> String {
        normalize_icon(&active_theme().context.glob)
    }
    pub fn ctx_grep() -> String {
        normalize_icon(&active_theme().context.grep)
    }
    pub fn ctx_tmux() -> String {
        normalize_icon(&active_theme().context.tmux)
    }
    pub fn ctx_git() -> String {
        normalize_icon(&active_theme().context.git)
    }
    pub fn ctx_scratchpad() -> String {
        normalize_icon(&active_theme().context.scratchpad)
    }
    pub fn ctx_library() -> String {
        normalize_icon(&active_theme().context.library)
    }
    pub fn ctx_skill() -> String {
        normalize_icon(&active_theme().context.skill)
    }
    pub fn ctx_spine() -> String {
        normalize_icon(&active_theme().context.spine)
    }

    // Message status (normalized to 2 cells)
    pub fn status_full() -> String {
        normalize_icon(&active_theme().status.full)
    }
    pub fn status_summarized() -> String {
        normalize_icon(&active_theme().status.summarized)
    }
    pub fn status_deleted() -> String {
        normalize_icon(&active_theme().status.deleted)
    }

    // Todo status (normalized to 2 cells)
    pub fn todo_pending() -> String {
        normalize_icon(&active_theme().todo.pending)
    }
    pub fn todo_in_progress() -> String {
        normalize_icon(&active_theme().todo.in_progress)
    }
    pub fn todo_done() -> String {
        normalize_icon(&active_theme().todo.done)
    }
}

// =============================================================================
// TOOL CATEGORY DESCRIPTIONS (loaded from yamls/ui.yaml via config module)
// =============================================================================

pub mod tool_categories {
    use crate::config::UI;

    pub fn file_desc() -> &'static str {
        &UI.tool_categories.file
    }
    pub fn tree_desc() -> &'static str {
        &UI.tool_categories.tree
    }
    pub fn console_desc() -> &'static str {
        &UI.tool_categories.console
    }
    pub fn context_desc() -> &'static str {
        &UI.tool_categories.context
    }
    pub fn todo_desc() -> &'static str {
        &UI.tool_categories.todo
    }
    pub fn memory_desc() -> &'static str {
        &UI.tool_categories.memory
    }
    pub fn git_desc() -> &'static str {
        &UI.tool_categories.git
    }
    pub fn scratchpad_desc() -> &'static str {
        &UI.tool_categories.scratchpad
    }
}

// =============================================================================
// PROMPTS (loaded from yamls/prompts.yaml via config module)
// =============================================================================

pub mod library {
    use crate::config::LIBRARY;

    pub fn default_agent_id() -> &'static str {
        &LIBRARY.default_agent_id
    }
    pub fn default_agent_content() -> &'static str {
        let id = &LIBRARY.default_agent_id;
        LIBRARY.agents.iter().find(|a| a.id == *id).map(|a| a.content.as_str()).unwrap_or("")
    }
    pub fn agents() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.agents
    }
    pub fn skills() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.skills
    }
    pub fn commands() -> &'static [crate::config::SeedEntry] {
        &LIBRARY.commands
    }
}

pub mod prompts {
    use crate::config::PROMPTS;

    pub fn tldr_prompt() -> &'static str {
        &PROMPTS.tldr_prompt
    }
    pub fn tldr_min_tokens() -> usize {
        PROMPTS.tldr_min_tokens
    }
    pub fn panel_header() -> &'static str {
        &PROMPTS.panel.header
    }
    pub fn panel_timestamp() -> &'static str {
        &PROMPTS.panel.timestamp
    }
    pub fn panel_timestamp_unknown() -> &'static str {
        &PROMPTS.panel.timestamp_unknown
    }
    pub fn panel_footer() -> &'static str {
        &PROMPTS.panel.footer
    }
    pub fn panel_footer_msg_line() -> &'static str {
        &PROMPTS.panel.footer_msg_line
    }
    pub fn panel_footer_msg_header() -> &'static str {
        &PROMPTS.panel.footer_msg_header
    }
    pub fn panel_footer_ack() -> &'static str {
        &PROMPTS.panel.footer_ack
    }
}
