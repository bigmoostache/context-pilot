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

/// Maximum cleaning iterations before forcing stop
pub const MAX_CLEANING_ITERATIONS: u32 = 10;

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
// ICONS / EMOJIS
// =============================================================================

pub mod icons {
    // Message types
    pub const MSG_USER: &str = "🦊";
    pub const MSG_ASSISTANT: &str = "🤖";
    pub const MSG_TOOL_CALL: &str = "🪵";
    pub const MSG_TOOL_RESULT: &str = "🔥";
    pub const MSG_ERROR: &str = "⚠️";

    // Context panel types
    pub const CTX_SYSTEM: &str = "🌱";
    pub const CTX_CONVERSATION: &str = "📜";
    pub const CTX_TREE: &str = "🌲";
    pub const CTX_TODO: &str = "🪓";
    pub const CTX_MEMORY: &str = "✨";
    pub const CTX_OVERVIEW: &str = "🌍";
    pub const CTX_FILE: &str = "💾";
    pub const CTX_GLOB: &str = "🔭";
    pub const CTX_GREP: &str = "👓";
    pub const CTX_TMUX: &str = "🕹️";
    pub const CTX_GIT: &str = "🐙";
    pub const CTX_SCRATCHPAD: &str = "🪶";

    // Message status
    pub const STATUS_FULL: &str = "";
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
// TOOL CATEGORY DESCRIPTIONS
// =============================================================================

pub mod tool_categories {
    /// Description for File tools category
    pub const FILE_DESC: &str = "Read, write, and search files in the project";
    
    /// Description for Tree tools category
    pub const TREE_DESC: &str = "Navigate and annotate the directory structure";
    
    /// Description for Console tools category
    pub const CONSOLE_DESC: &str = "Execute commands and monitor terminal output";
    
    /// Description for Context tools category
    pub const CONTEXT_DESC: &str = "Manage conversation context and system prompts";
    
    /// Description for Todo tools category
    pub const TODO_DESC: &str = "Track tasks and progress during the session";
    
    /// Description for Memory tools category
    pub const MEMORY_DESC: &str = "Store persistent memories across the conversation";
    
    /// Description for Git tools category
    pub const GIT_DESC: &str = "Version control operations and repository management";
    
    /// Description for Scratchpad tools category
    pub const SCRATCHPAD_DESC: &str = "A useful scratchpad for you to use however you like";
}

// =============================================================================
// PROMPTS
// =============================================================================

pub mod prompts {
    /// Default seed ID
    pub const DEFAULT_SEED_ID: &str = "S0";

    /// Default seed name
    pub const DEFAULT_SEED_NAME: &str = "Default";

    /// Default seed description
    pub const DEFAULT_SEED_DESC: &str = "Default coding assistant";

    /// Default seed content (main system prompt)
    pub const DEFAULT_SEED_CONTENT: &str = r#"You are a helpful coding assistant.

IMPORTANT: Messages in context have ID prefixes like [U1], [A1], [R1] for internal tracking.
These are for context management only - NEVER include these prefixes in your responses.
Just respond naturally without any [Axxx] or similar prefixes."#;

    /// Main system prompt for the assistant (alias for backward compatibility)
    pub const MAIN_SYSTEM: &str = DEFAULT_SEED_CONTENT;

    /// TL;DR summarization prompt
    pub const TLDR_PROMPT: &str = "Summarize the following message in 1-2 short sentences (max 50 words). Be concise and capture the key point:\n\n";

    /// Minimum token count to trigger LLM summarization (below this, use content directly)
    pub const TLDR_MIN_TOKENS: usize = 25;

    /// Header text for dynamic panel display (shown before panels)
    pub const PANEL_HEADER: &str = "Beginning of dynamic panel display. All content displayed below may be considered up to date.";

    /// Template for individual panel timestamp line
    /// Placeholders: {iso_time}, {time_delta}
    pub const PANEL_TIMESTAMP: &str = "Panel automatically generated at {iso_time} ({time_delta})";

    /// Fallback for panels with unknown/zero timestamp
    pub const PANEL_TIMESTAMP_UNKNOWN: &str = "Panel content (timestamp unknown - static or never refreshed)";

    /// Footer text template for dynamic panel display (shown after panels)
    /// Placeholders: {message_timestamps}, {current_datetime}
    pub const PANEL_FOOTER: &str = r#"End of dynamic panel displays. All content displayed above may be considered up to date: it is automatically kept updated as we speak.

{message_timestamps}
Current datetime: {current_datetime}"#;

    /// Template for each message timestamp line in footer
    /// Placeholders: {id}, {role}, {iso_time}, {time_delta}
    pub const PANEL_FOOTER_MSG_LINE: &str = "  - [{id}] {role}: {iso_time} ({time_delta})";

    /// Header for the message timestamps section in footer
    pub const PANEL_FOOTER_MSG_HEADER: &str = "Last message datetimes:";

    /// Text for panel footer tool result acknowledgment
    pub const PANEL_FOOTER_ACK: &str = "Panel display complete. Proceeding with conversation.";

    /// Context cleaner system prompt
    pub const CLEANER_SYSTEM: &str = r#"You are a context management assistant. Your ONLY job is to reduce context usage intelligently.

## CURRENT STATUS
- Current tokens: {current_tokens}
- Target tokens: {target_tokens}
- Tokens to remove: {tokens_to_remove}

Follow this structured approach:

## Phase 1: Understand Current Work
First, identify what the user is currently working on by examining:
- Recent messages (last 5-10 exchanges)
- Open files and their relevance
- Active todos
- Recent tool calls

Write a brief summary: "User is currently working on: ..."

## Phase 2: Map Context Importance
For EACH context element (P7+), assign an importance level relative to current work:

| ID | Type | Tokens | Importance | Reason |
|----|------|--------|------------|--------|
| P7 | file | 1234 | IRRELEVANT | Old lookup, not related to current task |
| P8 | glob | 567 | LOW | Search completed, found what was needed |
| ... | ... | ... | ... | ... |

Importance levels:
- **CRITICAL**: Actively being edited/discussed, essential to current task
- **HIGH**: Referenced recently, likely needed soon  
- **MEDIUM**: Related to current work but not immediately needed
- **LOW**: Tangentially related, opened for quick lookup
- **IRRELEVANT**: No longer related to current work, old explorations

## Phase 3: Calculate Reduction Strategy
Based on the mapping, plan which elements to remove to reach target:

"To remove {tokens_to_remove} tokens, I will:
1. Close P7 (file, 1234 tokens) - IRRELEVANT
2. Delete tool results T1-T5 (~500 tokens) - old executions
3. Summarize messages A1-A3 (~800 tokens) - verbose explanations
Total planned reduction: ~2534 tokens"

## Phase 4: Execute Cleanup
Make all necessary tool calls efficiently (batch when possible).

---

## Quick Wins (Easy Token Savings):

1. **Old tool calls (T#) and results (R#)** - DELETE these first via set_message_status, they're rarely needed after execution
2. **Summarize older assistant messages** - Keep key decisions, remove verbose explanations
3. **Close folders in P2 (Directory Tree)** - Use tree_toggle_folders to close irrelevant directories (saves tokens in tree output)
4. **Close glob/grep searches (P7+)** - Once you found what you needed, close them
5. **Close files opened for quick lookup** - If not actively editing, close it
6. **Delete completed todos** - Done items waste tokens
7. **Close tmux panes** - Terminal output is transient, close completed commands
8. **Remove outdated memories** - Delete memories about completed tasks

## Rules:
- NEVER close P1-P6 (core context elements)
- NEVER delete/summarize the last 5 messages (keep recent context)
- Prefer deleting over summarizing when content is truly obsolete
- Be aggressive with IRRELEVANT items, cautious with MEDIUM+
- After cleaning, report: what was removed and estimated tokens saved
"#;
}
