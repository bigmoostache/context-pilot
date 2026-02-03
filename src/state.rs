use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::rc::Rc;

use ratatui::text::Line;
use serde::{Deserialize, Serialize};

use crate::constants::icons;
use crate::llms::ModelInfo;
use crate::tool_defs::{ToolDefinition, get_all_tool_definitions};

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
}

impl ContextType {
    /// Returns true if this is a fixed/system context type
    pub fn is_fixed(&self) -> bool {
        matches!(self,
            ContextType::Conversation |
            ContextType::Tree |
            ContextType::Todo |
            ContextType::Memory |
            ContextType::Overview |
            ContextType::Git
        )
    }

    /// Get icon for this context type
    pub fn icon(&self) -> &'static str {
        match self {
            ContextType::Conversation => icons::CTX_CONVERSATION,
            ContextType::File => icons::CTX_FILE,
            ContextType::Tree => icons::CTX_TREE,
            ContextType::Glob => icons::CTX_GLOB,
            ContextType::Grep => icons::CTX_GREP,
            ContextType::Tmux => icons::CTX_TMUX,
            ContextType::Todo => icons::CTX_TODO,
            ContextType::Memory => icons::CTX_MEMORY,
            ContextType::Overview => icons::CTX_OVERVIEW,
            ContextType::Git => icons::CTX_GIT,
        }
    }

    /// Returns true if this context type uses cached_content from background loading
    pub fn needs_cache(&self) -> bool {
        matches!(self,
            ContextType::File |
            ContextType::Tree |
            ContextType::Glob |
            ContextType::Grep |
            ContextType::Tmux |
            ContextType::Git
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextElement {
    /// Context element ID (e.g., P1, P2, ...)
    pub id: String,
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
}

/// Todo item status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    Pending,    // ' '
    InProgress, // '~'
    Done,       // 'x'
}

impl TodoStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            TodoStatus::Pending => icons::TODO_PENDING,
            TodoStatus::InProgress => icons::TODO_IN_PROGRESS,
            TodoStatus::Done => icons::TODO_DONE,
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            " " | "pending" => Some(TodoStatus::Pending),
            "~" | "in_progress" => Some(TodoStatus::InProgress),
            "x" | "X" | "done" => Some(TodoStatus::Done),
            _ => None,
        }
    }
}

/// A todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Todo ID (X1, X2, ...)
    pub id: String,
    /// Parent todo ID (for nesting)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Todo name/title
    pub name: String,
    /// Detailed description
    #[serde(default)]
    pub description: String,
    /// Status: pending, in_progress, done
    #[serde(default)]
    pub status: TodoStatus,
}

/// Memory importance level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryImportance {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl MemoryImportance {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(MemoryImportance::Low),
            "medium" => Some(MemoryImportance::Medium),
            "high" => Some(MemoryImportance::High),
            "critical" => Some(MemoryImportance::Critical),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryImportance::Low => "low",
            MemoryImportance::Medium => "medium",
            MemoryImportance::High => "high",
            MemoryImportance::Critical => "critical",
        }
    }
}

/// A memory item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    /// Memory ID (M1, M2, ...)
    pub id: String,
    /// Memory content
    pub content: String,
    /// Importance level
    #[serde(default)]
    pub importance: MemoryImportance,
}

/// A file description in the tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeFileDescription {
    /// File path (relative to project root)
    pub path: String,
    /// Description of the file
    pub description: String,
    /// File hash when description was created (to detect staleness)
    pub file_hash: String,
}

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
    /// Message ID (e.g., U1, A1, T1)
    pub id: String,
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
}

/// Default tree filter (gitignore-style patterns)
pub const DEFAULT_TREE_FILTER: &str = r#"# Ignore common non-essential directories
.git/
target/
node_modules/
__pycache__/
.venv/
venv/
dist/
build/
*.pyc
*.pyo
.DS_Store
"#;

/// Persisted state (message IDs only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub context: Vec<ContextElement>,
    pub message_ids: Vec<String>,
    pub selected_context: usize,
    /// Draft input text (not yet sent)
    #[serde(default)]
    pub draft_input: String,
    /// Cursor position in draft input
    #[serde(default)]
    pub draft_cursor: usize,
    /// Gitignore-style filter for directory tree
    #[serde(default = "default_tree_filter")]
    pub tree_filter: String,
    /// Open folders in tree view (paths relative to project root)
    #[serde(default)]
    pub tree_open_folders: Vec<String>,
    /// File descriptions in tree view
    #[serde(default)]
    pub tree_descriptions: Vec<TreeFileDescription>,
    /// Next user message ID
    #[serde(default = "default_one")]
    pub next_user_id: usize,
    /// Next assistant message ID
    #[serde(default = "default_one")]
    pub next_assistant_id: usize,
    /// Next tool message ID
    #[serde(default = "default_one")]
    pub next_tool_id: usize,
    /// Next result message ID
    #[serde(default = "default_one")]
    pub next_result_id: usize,
    /// Todo items
    #[serde(default)]
    pub todos: Vec<TodoItem>,
    /// Next todo ID
    #[serde(default = "default_one")]
    pub next_todo_id: usize,
    /// Memory items
    #[serde(default)]
    pub memories: Vec<MemoryItem>,
    /// Next memory ID
    #[serde(default = "default_one")]
    pub next_memory_id: usize,
    /// Disabled tool IDs (tools not in this list are enabled)
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    /// Whether to show full diff content in Git panel
    #[serde(default = "default_true")]
    pub git_show_diffs: bool,
    /// PID of the process that owns this state (for preventing concurrent instances)
    #[serde(default)]
    pub owner_pid: Option<u32>,
    /// Dev mode - shows additional debug info like token counts
    #[serde(default)]
    pub dev_mode: bool,
    /// Selected LLM provider
    #[serde(default)]
    pub llm_provider: crate::llms::LlmProvider,
    /// Selected Anthropic model
    #[serde(default)]
    pub anthropic_model: crate::llms::AnthropicModel,
    /// Selected Grok model
    #[serde(default)]
    pub grok_model: crate::llms::GrokModel,
    /// Cleaning threshold (0.0 - 1.0), triggers auto-cleaning when exceeded
    #[serde(default = "default_cleaning_threshold")]
    pub cleaning_threshold: f32,
    /// Cleaning target as proportion of threshold (0.0 - 1.0)
    #[serde(default = "default_cleaning_target_proportion")]
    pub cleaning_target_proportion: f32,
    /// Context budget in tokens (None = use model's full context window)
    #[serde(default)]
    pub context_budget: Option<usize>,
}

fn default_cleaning_threshold() -> f32 {
    0.70
}

fn default_cleaning_target_proportion() -> f32 {
    0.70 // 70% of threshold
}

fn default_one() -> usize {
    1
}

fn default_true() -> bool {
    true
}

fn default_tree_filter() -> String {
    DEFAULT_TREE_FILTER.to_string()
}

/// Estimate tokens from text (~4 chars per token)
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
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
    /// Todo items
    pub todos: Vec<TodoItem>,
    /// Next todo ID (X1, X2, ...)
    pub next_todo_id: usize,
    /// Memory items
    pub memories: Vec<MemoryItem>,
    /// Next memory ID (M1, M2, ...)
    pub next_memory_id: usize,
    /// Tool definitions with enabled state
    pub tools: Vec<ToolDefinition>,
    /// Whether context cleaning is in progress
    pub is_cleaning_context: bool,
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
    /// Selected LLM provider
    pub llm_provider: crate::llms::LlmProvider,
    /// Selected Anthropic model
    pub anthropic_model: crate::llms::AnthropicModel,
    /// Selected Grok model
    pub grok_model: crate::llms::GrokModel,
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
    /// Whether we're in a git repository
    pub git_is_repo: bool,
    /// Per-file git changes
    pub git_file_changes: Vec<GitFileChange>,
    /// Last time git status was refreshed (milliseconds)
    pub git_last_refresh_ms: u64,
    /// Whether to show full diff content in Git panel (vs summary only)
    pub git_show_diffs: bool,
    /// Hash of last git status --porcelain output (for change detection)
    pub git_status_hash: Option<String>,
    /// Current API retry count (reset on success)
    pub api_retry_count: u32,

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

/// Represents a file change in git status
#[derive(Debug, Clone)]
pub struct GitFileChange {
    /// File path (relative to repo root)
    pub path: String,
    /// Lines added
    pub additions: i32,
    /// Lines deleted
    pub deletions: i32,
    /// Type of change
    pub change_type: GitChangeType,
    /// Diff content for this file (unified diff format)
    pub diff_content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChangeType {
    /// Modified file (staged or unstaged)
    Modified,
    /// Newly added/untracked file
    Added,
    /// Deleted file
    Deleted,
    /// Renamed file
    Renamed,
}

impl Default for State {
    fn default() -> Self {
        Self {
            context: vec![
                ContextElement {
                    id: "P1".to_string(),
                    context_type: ContextType::Conversation,
                    name: "Main".to_string(),
                    token_count: 0,
                    file_path: None,
                    file_hash: None,
                    glob_pattern: None,
                    glob_path: None,
                    grep_pattern: None,
                    grep_path: None,
                    grep_file_pattern: None,
                    tmux_pane_id: None,
                    tmux_lines: None,
                    tmux_last_keys: None,
                    tmux_description: None,
                    cached_content: None,
                    cache_deprecated: false,
                    last_refresh_ms: 0,
                    tmux_last_lines_hash: None,
                },
                ContextElement {
                    id: "P2".to_string(),
                    context_type: ContextType::Tree,
                    name: "Directory".to_string(),
                    token_count: 0,
                    file_path: None,
                    file_hash: None,
                    glob_pattern: None,
                    glob_path: None,
                    grep_pattern: None,
                    grep_path: None,
                    grep_file_pattern: None,
                    tmux_pane_id: None,
                    tmux_lines: None,
                    tmux_last_keys: None,
                    tmux_description: None,
                    cached_content: None,
                    cache_deprecated: true, // Initial refresh needed
                    last_refresh_ms: 0,
                    tmux_last_lines_hash: None,
                },
                ContextElement {
                    id: "P3".to_string(),
                    context_type: ContextType::Todo,
                    name: "Todo".to_string(),
                    token_count: 0,
                    file_path: None,
                    file_hash: None,
                    glob_pattern: None,
                    glob_path: None,
                    grep_pattern: None,
                    grep_path: None,
                    grep_file_pattern: None,
                    tmux_pane_id: None,
                    tmux_lines: None,
                    tmux_last_keys: None,
                    tmux_description: None,
                    cached_content: None,
                    cache_deprecated: false,
                    last_refresh_ms: 0,
                    tmux_last_lines_hash: None,
                },
                ContextElement {
                    id: "P4".to_string(),
                    context_type: ContextType::Memory,
                    name: "Memory".to_string(),
                    token_count: 0,
                    file_path: None,
                    file_hash: None,
                    glob_pattern: None,
                    glob_path: None,
                    grep_pattern: None,
                    grep_path: None,
                    grep_file_pattern: None,
                    tmux_pane_id: None,
                    tmux_lines: None,
                    tmux_last_keys: None,
                    tmux_description: None,
                    cached_content: None,
                    cache_deprecated: false,
                    last_refresh_ms: 0,
                    tmux_last_lines_hash: None,
                },
                ContextElement {
                    id: "P5".to_string(),
                    context_type: ContextType::Overview,
                    name: "Overview".to_string(),
                    token_count: 0,
                    file_path: None,
                    file_hash: None,
                    glob_pattern: None,
                    glob_path: None,
                    grep_pattern: None,
                    grep_path: None,
                    grep_file_pattern: None,
                    tmux_pane_id: None,
                    tmux_lines: None,
                    tmux_last_keys: None,
                    tmux_description: None,
                    cached_content: None,
                    cache_deprecated: false,
                    last_refresh_ms: 0,
                    tmux_last_lines_hash: None,
                },
                ContextElement {
                    id: "P6".to_string(),
                    context_type: ContextType::Git,
                    name: "Git".to_string(),
                    token_count: 0,
                    file_path: None,
                    file_hash: None,
                    glob_pattern: None,
                    glob_path: None,
                    grep_pattern: None,
                    grep_path: None,
                    grep_file_pattern: None,
                    tmux_pane_id: None,
                    tmux_lines: None,
                    tmux_last_keys: None,
                    tmux_description: None,
                    cached_content: None,
                    cache_deprecated: false,
                    last_refresh_ms: 0,
                    tmux_last_lines_hash: None,
                },
            ],
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
            todos: vec![],
            next_todo_id: 1,
            memories: vec![],
            next_memory_id: 1,
            tools: get_all_tool_definitions(),
            is_cleaning_context: false,
            dirty: true, // Start dirty to ensure initial render
            spinner_frame: 0,
            dev_mode: false,
            perf_enabled: false,
            config_view: false,
            config_selected_bar: 0,
            llm_provider: crate::llms::LlmProvider::default(),
            anthropic_model: crate::llms::AnthropicModel::default(),
            grok_model: crate::llms::GrokModel::default(),
            cleaning_threshold: 0.70,
            cleaning_target_proportion: 0.70,
            context_budget: None, // Use model's full context window
            // API check defaults
            api_check_in_progress: false,
            api_check_result: None,
            // Git status defaults
            git_branch: None,
            git_is_repo: false,
            git_file_changes: vec![],
            git_last_refresh_ms: 0,
            git_show_diffs: true, // Show diffs by default
            git_status_hash: None,
            // API retry
            api_retry_count: 0,
            // Render cache
            last_viewport_width: 0,
            message_cache: HashMap::new(),
            input_cache: None,
            full_content_cache: None,
        }
    }
}

impl State {
    /// Find the first available context ID (fills gaps instead of always incrementing)
    pub fn next_available_context_id(&self) -> String {
        // Collect all existing numeric IDs
        let used_ids: std::collections::HashSet<usize> = self.context.iter()
            .filter_map(|c| c.id.strip_prefix('P').and_then(|n| n.parse().ok()))
            .collect();

        // Find first available starting from 7 (P1-P6 are fixed defaults)
        let id = (7..).find(|n| !used_ids.contains(n)).unwrap_or(7);
        format!("P{}", id)
    }

    /// Get the API model string for the current provider/model selection
    pub fn current_model(&self) -> String {
        match self.llm_provider {
            crate::llms::LlmProvider::Anthropic | crate::llms::LlmProvider::ClaudeCode => {
                self.anthropic_model.api_name().to_string()
            }
            crate::llms::LlmProvider::Grok => self.grok_model.api_name().to_string(),
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
