use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::rc::Rc;

use ratatui::text::Line;
use serde::{Deserialize, Serialize};

use crate::constants::{icons, CHARS_PER_TOKEN};
use crate::llms::ModelInfo;
use crate::tool_defs::ToolDefinition;

/// Cached rendered lines for a message (using Rc to avoid clones)
#[derive(Clone)]
pub struct MessageRenderCache {
    /// Pre-rendered lines for this message
    pub lines: Rc<Vec<Line<'static>>>,
    /// Hash of content that affects rendering
    pub content_hash: u64,
    /// Viewport width used for wrapping
    pub viewport_width: u16,
}

/// Cached rendered lines for input area (using Rc to avoid clones)
#[derive(Clone)]
pub struct InputRenderCache {
    /// Pre-rendered lines for input
    pub lines: Rc<Vec<Line<'static>>>,
    /// Hash of input + cursor position
    pub input_hash: u64,
    /// Viewport width used for wrapping
    pub viewport_width: u16,
}

/// Top-level cache for entire conversation content
#[derive(Clone)]
pub struct FullContentCache {
    /// Complete rendered output
    pub lines: Rc<Vec<Line<'static>>>,
    /// Hash of all inputs that affect rendering
    pub content_hash: u64,
}

/// Hash helper for cache invalidation
pub fn hash_values<T: Hash>(values: &[T]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for v in values {
        v.hash(&mut hasher);
    }
    hasher.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextType {
    System,
    Conversation,
    File,
    Tree,
    Glob,
    Grep,
    Tmux,
    Todo,
    Memory,
    Overview,
    Git,
    Scratchpad,
}

impl ContextType {
    /// Returns true if this is a fixed/system context type (derived from module registry)
    pub fn is_fixed(&self) -> bool {
        crate::modules::all_modules().iter().any(|m| m.fixed_panel_types().contains(self))
    }

    /// Get icon for this context type (normalized to 2 cells)
    pub fn icon(&self) -> String {
        match self {
            ContextType::System => icons::ctx_system(),
            ContextType::Conversation => icons::ctx_conversation(),
            ContextType::File => icons::ctx_file(),
            ContextType::Tree => icons::ctx_tree(),
            ContextType::Glob => icons::ctx_glob(),
            ContextType::Grep => icons::ctx_grep(),
            ContextType::Tmux => icons::ctx_tmux(),
            ContextType::Todo => icons::ctx_todo(),
            ContextType::Memory => icons::ctx_memory(),
            ContextType::Overview => icons::ctx_overview(),
            ContextType::Git => icons::ctx_git(),
            ContextType::Scratchpad => icons::ctx_scratchpad(),
        }
    }

    /// Returns true if this context type uses cached_content from background loading.
    /// Delegates to the Panel trait's needs_cache() method.
    pub fn needs_cache(&self) -> bool {
        crate::modules::create_panel(*self)
            .map(|p| p.needs_cache())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextElement {
    /// Display ID (e.g., P1, P2, ... for UI/LLM)
    pub id: String,
    /// UID for dynamic panels (None for fixed P1-P7, Some for P8+)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    pub context_type: ContextType,
    pub name: String,
    pub token_count: usize,
    /// File path (for File context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// File content hash (for change detection - not persisted)
    #[serde(skip)]
    pub file_hash: Option<String>,
    /// Glob pattern (for Glob context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_pattern: Option<String>,
    /// Glob search path (for Glob context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_path: Option<String>,
    /// Grep regex pattern (for Grep context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_pattern: Option<String>,
    /// Grep search path (for Grep context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_path: Option<String>,
    /// Grep file filter pattern (for Grep context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_file_pattern: Option<String>,
    /// Tmux pane ID (for Tmux context type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_pane_id: Option<String>,
    /// Number of lines to capture from tmux pane
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_lines: Option<usize>,
    /// Last keys sent to this tmux pane
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_last_keys: Option<String>,
    /// Description of what this tmux pane is for
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_description: Option<String>,

    // === Caching fields (not persisted) ===
    /// Cached content for LLM context and UI rendering
    #[serde(skip)]
    pub cached_content: Option<String>,
    /// Cache is deprecated - source data changed, needs regeneration
    #[serde(skip)]
    pub cache_deprecated: bool,
    /// Last time this element was refreshed (for timer-based deprecation)
    #[serde(skip)]
    pub last_refresh_ms: u64,
    /// Hash of tmux last 2 lines (for change detection)
    #[serde(skip)]
    pub tmux_last_lines_hash: Option<String>,
    /// Current page (0-indexed) for LLM context pagination
    #[serde(skip)]
    pub current_page: usize,
    /// Total pages for LLM context pagination
    #[serde(skip)]
    pub total_pages: usize,
    /// Full content token count (before pagination). token_count reflects current page.
    #[serde(skip)]
    pub full_token_count: usize,
}

// Re-export module-owned types for backwards compatibility
pub use crate::modules::todo::types::{TodoStatus, TodoItem};
pub use crate::modules::memory::types::{MemoryImportance, MemoryItem};
pub use crate::modules::prompt::types::SystemItem;
pub use crate::modules::scratchpad::types::ScratchpadCell;
pub use crate::modules::tree::types::{TreeFileDescription, DEFAULT_TREE_FILTER};
pub use crate::modules::git::types::{GitFileChange, GitChangeType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    TextMessage,
    ToolCall,
    ToolResult,
}

impl Default for MessageType {
    fn default() -> Self {
        Self::TextMessage
    }
}

/// Message status for context management
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    #[default]
    Full,
    Summarized,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseRecord {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub tool_use_id: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Display ID (e.g., U1, A1, T1 - for UI/LLM)
    pub id: String,
    /// Internal UID (e.g., UID_42_U - never shown to UI/LLM)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    pub role: String,
    #[serde(default)]
    pub message_type: MessageType,
    pub content: String,
    #[serde(default)]
    pub content_token_count: usize,
    #[serde(default)]
    pub tl_dr: Option<String>,
    #[serde(default)]
    pub tl_dr_token_count: usize,
    /// Message status for context management
    #[serde(default)]
    pub status: MessageStatus,
    /// Tool uses in this message (for assistant messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_uses: Vec<ToolUseRecord>,
    /// Tool results in this message (for ToolResult messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResultRecord>,
    /// Input tokens used for this response (from API, for assistant messages)
    #[serde(default)]
    pub input_tokens: usize,
    /// Timestamp when this message was created (ms since UNIX epoch)
    #[serde(default)]
    pub timestamp_ms: u64,
}

// =============================================================================
// MULTI-WORKER STATE STRUCTS
// =============================================================================

/// Shared configuration (config.json)
/// Infrastructure fields + module data under "modules" key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedConfig {
    // === Infrastructure ===
    /// Flag to request reload (checked by run.sh supervisor)
    #[serde(default)]
    pub reload_requested: bool,
    /// Active theme ID
    #[serde(default = "default_theme")]
    pub active_theme: String,
    /// PID of the process that owns this state
    #[serde(default)]
    pub owner_pid: Option<u32>,
    /// Selected context index
    #[serde(default)]
    pub selected_context: usize,
    /// Draft input text (not yet sent)
    #[serde(default)]
    pub draft_input: String,
    /// Cursor position in draft input
    #[serde(default)]
    pub draft_cursor: usize,

    // === Module data (keyed by module ID) ===
    #[serde(default)]
    pub modules: HashMap<String, serde_json::Value>,
}

impl Default for SharedConfig {
    fn default() -> Self {
        Self {
            reload_requested: false,
            active_theme: crate::config::DEFAULT_THEME.to_string(),
            owner_pid: None,
            selected_context: 0,
            draft_input: String::new(),
            draft_cursor: 0,
            modules: HashMap::new(),
        }
    }
}

/// Worker-specific state (states/{worker}.json)
/// Infrastructure fields + module data under "modules" key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerState {
    /// Worker identifier
    pub worker_id: String,

    // === Panel UIDs ===
    /// UIDs of important/fixed panels this worker uses
    #[serde(default)]
    pub important_panel_uids: ImportantPanelUids,
    /// Maps panel UIDs to local display IDs (excluding chat which is in important_panel_uids)
    #[serde(default)]
    pub panel_uid_to_local_id: HashMap<String, String>,

    // === Local ID counters ===
    /// Next tool message ID
    #[serde(default = "default_one")]
    pub next_tool_id: usize,
    /// Next result message ID
    #[serde(default = "default_one")]
    pub next_result_id: usize,

    // === Module data (keyed by module ID) ===
    #[serde(default)]
    pub modules: HashMap<String, serde_json::Value>,
}

impl Default for WorkerState {
    fn default() -> Self {
        Self {
            worker_id: crate::constants::DEFAULT_WORKER_ID.to_string(),
            important_panel_uids: HashMap::new(),
            panel_uid_to_local_id: HashMap::new(),
            next_tool_id: 1,
            next_result_id: 1,
            modules: HashMap::new(),
        }
    }
}

/// Panel data stored in panels/{uid}.json
/// All panels are stored here - fixed (System, Conversation, Tree, etc.) and dynamic (File, Glob, Grep, Tmux)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelData {
    /// UID of this panel
    pub uid: String,
    /// Panel type
    pub panel_type: ContextType,
    /// Display name
    pub name: String,
    /// Token count (preserved across sessions)
    #[serde(default)]
    pub token_count: usize,
    /// Last refresh timestamp in milliseconds (preserved across sessions)
    #[serde(default)]
    pub last_refresh_ms: u64,

    // === Conversation panel data ===
    /// Message UIDs for conversation panels
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_uids: Vec<String>,

    // === File/Glob/Grep/Tmux panel metadata ===
    /// File path (for File context)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Glob pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_pattern: Option<String>,
    /// Glob search path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_path: Option<String>,
    /// Grep regex pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_pattern: Option<String>,
    /// Grep search path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_path: Option<String>,
    /// Grep file filter pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_file_pattern: Option<String>,
    /// Tmux pane ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_pane_id: Option<String>,
    /// Number of lines to capture from tmux pane
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_lines: Option<usize>,
    /// Tmux pane description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_description: Option<String>,
}

/// UIDs for important/fixed panels that a worker uses.
/// Maps ContextType to panel UID string.
pub type ImportantPanelUids = HashMap<ContextType, String>;

// DEFAULT_TREE_FILTER is re-exported from modules/tree/types.rs above

fn default_theme() -> String {
    crate::config::DEFAULT_THEME.to_string()
}

fn default_one() -> usize {
    1
}

/// Estimate tokens from text (uses CHARS_PER_TOKEN constant)
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() as f32 / CHARS_PER_TOKEN).ceil() as usize
}

/// Compute total pages for a given token count using PANEL_PAGE_TOKENS
pub fn compute_total_pages(token_count: usize) -> usize {
    let max = crate::constants::PANEL_PAGE_TOKENS;
    if token_count <= max { 1 } else { (token_count + max - 1) / max }
}

/// Runtime state (messages loaded in memory)
pub struct State {
    pub context: Vec<ContextElement>,
    pub messages: Vec<Message>,
    pub input: String,
    /// Cursor position in input (byte index)
    pub input_cursor: usize,
    pub selected_context: usize,
    pub is_streaming: bool,
    pub scroll_offset: f32,
    pub user_scrolled: bool,
    /// Scroll acceleration (increases when holding scroll keys)
    pub scroll_accel: f32,
    /// Maximum scroll offset (set by UI based on content height)
    pub max_scroll: f32,
    /// Estimated tokens added during current streaming session (for correction when done)
    pub streaming_estimated_tokens: usize,
    /// Gitignore-style filter for directory tree
    pub tree_filter: String,
    /// Open folders in tree view (paths relative to project root)
    pub tree_open_folders: Vec<String>,
    /// File descriptions in tree view
    pub tree_descriptions: Vec<TreeFileDescription>,
    /// Number of pending TL;DR background jobs
    pub pending_tldrs: usize,
    /// Next user message ID (U1, U2, ...)
    pub next_user_id: usize,
    /// Next assistant message ID (A1, A2, ...)
    pub next_assistant_id: usize,
    /// Next tool message ID (T1, T2, ...)
    pub next_tool_id: usize,
    /// Next result message ID (R1, R2, ...)
    pub next_result_id: usize,
    /// Global UID counter for all shared elements (messages, panels)
    pub global_next_uid: usize,
    /// Todo items
    pub todos: Vec<TodoItem>,
    /// Next todo ID (X1, X2, ...)
    pub next_todo_id: usize,
    /// Memory items
    pub memories: Vec<MemoryItem>,
    /// Next memory ID (M1, M2, ...)
    pub next_memory_id: usize,
    /// System prompt items
    pub systems: Vec<SystemItem>,
    /// Next system ID (S0, S1, ...)
    pub next_system_id: usize,
    /// Active system ID (None = default)
    pub active_system_id: Option<String>,
    /// Scratchpad cells
    pub scratchpad_cells: Vec<ScratchpadCell>,
    /// Next scratchpad cell ID (C1, C2, ...)
    pub next_scratchpad_id: usize,
    /// Tool definitions with enabled state
    pub tools: Vec<ToolDefinition>,
    /// Active module IDs
    pub active_modules: std::collections::HashSet<String>,
    /// Whether the UI needs to be redrawn
    pub dirty: bool,
    /// Frame counter for spinner animations (wraps around)
    pub spinner_frame: u64,
    /// Dev mode - shows additional debug info like token counts
    pub dev_mode: bool,
    /// Performance monitoring overlay enabled (F12 to toggle)
    pub perf_enabled: bool,
    /// Configuration view is open (Ctrl+H to toggle)
    pub config_view: bool,
    /// Selected bar in config view (0=budget, 1=threshold, 2=target)
    pub config_selected_bar: usize,
    /// Active theme ID (dnd, modern, futuristic, forest, sea, space)
    pub active_theme: String,
    /// Selected LLM provider
    pub llm_provider: crate::llms::LlmProvider,
    /// Selected Anthropic model
    pub anthropic_model: crate::llms::AnthropicModel,
    /// Selected Grok model
    pub grok_model: crate::llms::GrokModel,
    /// Selected Groq model
    pub groq_model: crate::llms::GroqModel,
    /// Cleaning threshold (0.0 - 1.0), triggers auto-cleaning when exceeded
    pub cleaning_threshold: f32,
    /// Cleaning target as proportion of threshold (0.0 - 1.0)
    pub cleaning_target_proportion: f32,
    /// Context budget in tokens (None = use model's full context window)
    pub context_budget: Option<usize>,

    // === API Check Status (runtime-only) ===
    /// Whether an API check is in progress
    pub api_check_in_progress: bool,
    /// Result of the last API check
    pub api_check_result: Option<crate::llms::ApiCheckResult>,

    // === Git Status (runtime-only, not persisted) ===
    /// Current git branch name (None if not a git repo)
    pub git_branch: Option<String>,
    /// All local branches (name, is_current)
    pub git_branches: Vec<(String, bool)>,
    /// Whether we're in a git repository
    pub git_is_repo: bool,
    /// Per-file git changes
    pub git_file_changes: Vec<GitFileChange>,
    /// Whether to show full diff content in Git panel (vs summary only)
    pub git_show_diffs: bool,
    /// Hash of last git status --porcelain output (for change detection)
    pub git_status_hash: Option<String>,
    /// Whether to show git log in Git panel
    pub git_show_logs: bool,
    /// Custom git log arguments (e.g., "-5 --oneline")
    pub git_log_args: Option<String>,
    /// Cached git log output
    pub git_log_content: Option<String>,
    /// Current API retry count (reset on success)
    pub api_retry_count: u32,
    /// Reload pending flag (set by system_reload tool, triggers reload after tool result is saved)
    pub reload_pending: bool,
    /// Waiting for file panels to load before continuing stream
    pub waiting_for_panels: bool,

    // === Render Cache (runtime-only) ===
    /// Last viewport width (for pre-wrapping text)
    pub last_viewport_width: u16,
    /// Cached rendered lines per message ID
    pub message_cache: HashMap<String, MessageRenderCache>,
    /// Cached rendered lines for input area
    pub input_cache: Option<InputRenderCache>,
    /// Full content cache (entire conversation output)
    pub full_content_cache: Option<FullContentCache>,
}

// GitFileChange and GitChangeType are re-exported from modules/git/types.rs above

impl Default for State {
    fn default() -> Self {
        Self {
            context: crate::modules::all_fixed_panel_defaults()
                .iter()
                .enumerate()
                .map(|(i, (_, _, ct, name, cache_dep))| {
                    crate::modules::make_default_context_element(
                        &format!("P{}", i), *ct, name, *cache_dep,
                    )
                })
                .collect(),
            messages: vec![],
            input: String::new(),
            input_cursor: 0,
            selected_context: 0,
            is_streaming: false,
            scroll_offset: 0.0,
            user_scrolled: false,
            scroll_accel: 1.0,
            max_scroll: 0.0,
            streaming_estimated_tokens: 0,
            tree_filter: DEFAULT_TREE_FILTER.to_string(),
            tree_open_folders: vec![".".to_string()], // Root open by default
            tree_descriptions: vec![],
            pending_tldrs: 0,
            next_user_id: 1,
            next_assistant_id: 1,
            next_tool_id: 1,
            next_result_id: 1,
            global_next_uid: 1,
            todos: vec![],
            next_todo_id: 1,
            memories: vec![],
            next_memory_id: 1,
            systems: vec![],
            next_system_id: 0,
            active_system_id: None,
            scratchpad_cells: vec![],
            next_scratchpad_id: 1,
            active_modules: crate::modules::default_active_modules(),
            tools: crate::modules::active_tool_definitions(&crate::modules::default_active_modules()),
            dirty: true, // Start dirty to ensure initial render
            spinner_frame: 0,
            dev_mode: false,
            perf_enabled: false,
            config_view: false,
            config_selected_bar: 0,
            active_theme: crate::config::DEFAULT_THEME.to_string(),
            llm_provider: crate::llms::LlmProvider::default(),
            anthropic_model: crate::llms::AnthropicModel::default(),
            grok_model: crate::llms::GrokModel::default(),
            groq_model: crate::llms::GroqModel::default(),
            cleaning_threshold: 0.70,
            cleaning_target_proportion: 0.70,
            context_budget: None, // Use model's full context window
            // API check defaults
            api_check_in_progress: false,
            api_check_result: None,
            // Git status defaults
            git_branch: None,
            git_branches: vec![],
            git_is_repo: false,
            git_file_changes: vec![],
            git_show_diffs: true, // Show diffs by default
            git_status_hash: None,
            git_show_logs: false,
            git_log_args: None,
            git_log_content: None,
            // API retry
            api_retry_count: 0,
            reload_pending: false,
            waiting_for_panels: false,
            // Render cache
            last_viewport_width: 0,
            message_cache: HashMap::new(),
            input_cache: None,
            full_content_cache: None,
        }
    }
}

impl State {
    /// Update the last_refresh_ms timestamp for a panel by its context type
    pub fn touch_panel(&mut self, context_type: ContextType) {
        if let Some(ctx) = self.context.iter_mut().find(|c| c.context_type == context_type) {
            ctx.last_refresh_ms = crate::core::panels::now_ms();
        }
    }

    /// Find the first available context ID (fills gaps instead of always incrementing)
    pub fn next_available_context_id(&self) -> String {
        // Collect all existing numeric IDs
        let used_ids: std::collections::HashSet<usize> = self.context.iter()
            .filter_map(|c| c.id.strip_prefix('P').and_then(|n| n.parse().ok()))
            .collect();

        // Find first available starting from 8 (P0-P7 are fixed defaults)
        let id = (8..).find(|n| !used_ids.contains(n)).unwrap_or(8);
        format!("P{}", id)
    }

    /// Get the API model string for the current provider/model selection
    pub fn current_model(&self) -> String {
        match self.llm_provider {
            crate::llms::LlmProvider::Anthropic | crate::llms::LlmProvider::ClaudeCode => {
                self.anthropic_model.api_name().to_string()
            }
            crate::llms::LlmProvider::Grok => self.grok_model.api_name().to_string(),
            crate::llms::LlmProvider::Groq => self.groq_model.api_name().to_string(),
        }
    }

    /// Get the cleaning target as absolute proportion (threshold * target_proportion)
    pub fn cleaning_target(&self) -> f32 {
        self.cleaning_threshold * self.cleaning_target_proportion
    }

    /// Get the current model's context window
    pub fn model_context_window(&self) -> usize {
        match self.llm_provider {
            crate::llms::LlmProvider::Anthropic | crate::llms::LlmProvider::ClaudeCode => {
                self.anthropic_model.context_window()
            }
            crate::llms::LlmProvider::Grok => self.grok_model.context_window(),
            crate::llms::LlmProvider::Groq => self.groq_model.context_window(),
        }
    }

    /// Get effective context budget (custom or model's full context)
    pub fn effective_context_budget(&self) -> usize {
        self.context_budget.unwrap_or_else(|| self.model_context_window())
    }

    /// Get cleaning threshold in tokens
    pub fn cleaning_threshold_tokens(&self) -> usize {
        (self.effective_context_budget() as f32 * self.cleaning_threshold) as usize
    }

    /// Get cleaning target in tokens
    pub fn cleaning_target_tokens(&self) -> usize {
        (self.effective_context_budget() as f32 * self.cleaning_target()) as usize
    }
}
