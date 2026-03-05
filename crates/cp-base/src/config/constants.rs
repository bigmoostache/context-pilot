// =============================================================================
// API & MODELS
// =============================================================================

/// Maximum tokens for main response
pub const MAX_RESPONSE_TOKENS: u32 = 0x4000;

/// Anthropic API endpoint
pub const API_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// Anthropic API version
pub const API_VERSION: &str = "2023-06-01";

// =============================================================================
// CONTEXT & TOKEN MANAGEMENT
// =============================================================================

/// Average characters per token for token estimation
pub const CHARS_PER_TOKEN: f32 = 3.3;

/// Minimum active messages in a chunk before it can be detached.
pub const DETACH_CHUNK_MIN_MESSAGES: usize = 25;

/// Minimum token count in a chunk before it can be detached.
pub const DETACH_CHUNK_MIN_TOKENS: usize = 5_000;

/// Minimum messages to keep in the live conversation after detachment.
pub const DETACH_KEEP_MIN_MESSAGES: usize = 20;

/// Minimum tokens to keep in the live conversation after detachment.
pub const DETACH_KEEP_MIN_TOKENS: usize = 3_500;

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

// =============================================================================
// PANEL SIZE LIMITS
// =============================================================================

/// Hard cap: refuse to load any panel content larger than this (bytes)
pub const PANEL_MAX_LOAD_BYTES: usize = 5 * 1024 * 1024; // 5 MB

/// Tokens per page when paginating (also serves as the soft cap — panels exceeding this get paginated)
pub const PANEL_PAGE_TOKENS: usize = 25_000;

/// Maximum size for command output cached in result panels (bytes)
pub const MAX_RESULT_CONTENT_BYTES: usize = 1_000_000; // 1 MB

// =============================================================================
// SHARED DIRECTORY (version-controlled part of .context-pilot)
// =============================================================================

/// The shared directory path within .context-pilot.
pub const SHARED_DIR: &str = ".context-pilot/shared";

/// Ensure the `.context-pilot/shared/` directory exists and is un-gitignored.
///
/// If a `.gitignore` file exists at the project root and contains a rule that
/// ignores `.context-pilot`, this function appends `!.context-pilot/shared/`
/// as an exception — unless the exception already exists.
///
/// Call this at module init time for any module that uses shared assets.
pub fn ensure_shared_dir() {
    let _r = std::fs::create_dir_all(SHARED_DIR);
    ensure_gitignore_exception_at(std::path::Path::new(".gitignore"));
}

/// Check if a gitignore file ignores .context-pilot and add a shared/ exception if needed.
fn ensure_gitignore_exception_at(gitignore_path: &std::path::Path) {
    if !gitignore_path.exists() {
        return;
    }

    let Ok(content) = std::fs::read_to_string(gitignore_path) else { return };

    let ignores_cp = content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == ".context-pilot"
            || trimmed == "/.context-pilot"
            || trimmed == ".context-pilot/"
            || trimmed == "/.context-pilot/"
    });

    if !ignores_cp {
        return;
    }

    let has_exception = content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "!.context-pilot/shared/"
            || trimmed == "!.context-pilot/shared"
            || trimmed == "!/.context-pilot/shared/"
            || trimmed == "!/.context-pilot/shared"
    });

    if has_exception {
        return;
    }

    let mut new_content = content;
    if !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str("!.context-pilot/shared/\n");

    let _r = std::fs::write(gitignore_path, new_content);
}
