// =============================================================================
// API & MODELS
// =============================================================================

/// Main model for streaming responses
pub const MODEL_MAIN: &str = "claude-opus-4-5";

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

/// Maximum context size in tokens
pub const MAX_CONTEXT_TOKENS: usize = 200_000;

/// Threshold percentage to trigger automatic context cleaning (0.0 - 1.0)
pub const CLEANING_THRESHOLD: f32 = 0.70;

/// Target percentage to stop cleaning (0.0 - 1.0)
pub const CLEANING_TARGET: f32 = 0.50;

/// Maximum cleaning iterations before forcing stop
pub const MAX_CLEANING_ITERATIONS: u32 = 10;

// =============================================================================
// PANEL CACHE DEPRECATION
// =============================================================================

/// Deprecation timer for glob panels (milliseconds)
pub const GLOB_DEPRECATION_MS: u64 = 30_000; // 30 seconds

/// Deprecation timer for grep panels (milliseconds)
pub const GREP_DEPRECATION_MS: u64 = 30_000; // 30 seconds

/// Deprecation timer for tmux panels (milliseconds)
pub const TMUX_DEPRECATION_MS: u64 = 1_000; // 1 second (check hash of last 2 lines)

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
pub const SIDEBAR_HELP_HEIGHT: u16 = 7;

// =============================================================================
// EVENT LOOP
// =============================================================================

/// Poll interval for events in milliseconds
pub const EVENT_POLL_MS: u64 = 8;

/// Delay after tmux send-keys in milliseconds (allows command output to appear)
pub const TMUX_SEND_DELAY_MS: u64 = 2000;

/// Fixed sleep duration in seconds for the sleep tool
pub const SLEEP_DURATION_SECS: u64 = 2;

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
// ICONS / EMOJIS
// =============================================================================

pub mod icons {
    // Message types
    pub const MSG_USER: &str = "👤";
    pub const MSG_ASSISTANT: &str = "🤖";
    pub const MSG_TOOL_CALL: &str = "⚡";
    pub const MSG_TOOL_RESULT: &str = "🪛";
    pub const MSG_ERROR: &str = "⚠️";

    // Context panel types
    pub const CTX_CONVERSATION: &str = "💬";
    pub const CTX_TREE: &str = "📁";
    pub const CTX_TODO: &str = "✅";
    pub const CTX_MEMORY: &str = "🧠";
    pub const CTX_OVERVIEW: &str = "📊";
    pub const CTX_FILE: &str = "📄";
    pub const CTX_GLOB: &str = "🔍";
    pub const CTX_GREP: &str = "🔎";
    pub const CTX_TMUX: &str = "🎮";

    // Message status
    pub const STATUS_FULL: &str = "●";
    pub const STATUS_SUMMARIZED: &str = "◐";
    pub const STATUS_DELETED: &str = "○";

    // Todo status
    pub const TODO_PENDING: &str = "○";
    pub const TODO_IN_PROGRESS: &str = "◐";
    pub const TODO_DONE: &str = "●";

    // Context size indicators
    pub const SIZE_LARGE: &str = "🔴";
    pub const SIZE_MEDIUM: &str = "🟡";
    pub const SIZE_SMALL: &str = "🟢";
}

// =============================================================================
// PROMPTS
// =============================================================================

pub mod prompts {
    /// Main system prompt for the assistant
    pub const MAIN_SYSTEM: &str = r#"You are a helpful coding assistant.

IMPORTANT: Messages in context have ID prefixes like [U1], [A1], [R1] for internal tracking.
These are for context management only - NEVER include these prefixes in your responses.
Just respond naturally without any [Axxx] or similar prefixes."#;

    /// TL;DR summarization prompt
    pub const TLDR_PROMPT: &str = "Summarize the following message in 1-2 short sentences (max 50 words). Be concise and capture the key point:\n\n";

    /// Minimum token count to trigger LLM summarization (below this, use content directly)
    pub const TLDR_MIN_TOKENS: usize = 25;

    /// Context cleaner system prompt
    pub const CLEANER_SYSTEM: &str = r#"You are a context management assistant. Your ONLY job is to reduce context usage intelligently.

Current context is above 70% capacity and needs to be reduced.

## Strategy Priority (high to low impact):

1. **Close large file contexts (P7+)** - Files often consume the most tokens
   - Close files that haven't been referenced recently
   - Close files that were only opened for quick lookup
   - Keep files actively being edited

2. **Summarize or delete old messages** - Conversation history grows fast
   - DELETE: Old tool calls/results that are no longer relevant
   - DELETE: Superseded discussions (e.g., old approaches that were abandoned)
   - SUMMARIZE: Long assistant responses - keep key decisions only
   - SUMMARIZE: Long user messages with detailed context already acted upon
   - Keep recent messages (last 5-10 exchanges) at full status

3. **Close glob searches** - Often opened for exploration then deleted
   - Close globs that found what was needed
   - Close globs with too many results

4. **Close tmux panes** - Terminal output is often transient
   - Close panes for completed commands
   - Keep panes for ongoing processes

5. **Delete completed todos** - Done items waste tokens
   - Delete all todos with status 'done'
   - Consider deleting obsolete pending todos

6. **Clean up memories** - Remove outdated information
   - Delete memories about completed tasks
   - Delete memories superseded by newer ones

## Rules:
- Be aggressive but smart - aim to reduce by 30-50%
- NEVER close P1-P6 (core context elements)
- Prefer deleting over summarizing when content is truly obsolete
- Make multiple tool calls in one response for efficiency
- After cleaning, briefly report what was removed
"#;
}
