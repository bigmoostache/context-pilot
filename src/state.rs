use serde::{Deserialize, Serialize};

use crate::constants::icons;
use crate::tool_defs::{ToolDefinition, get_all_tool_definitions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl ContextType {
    /// Returns true if this is a fixed/system context type
    pub fn is_fixed(&self) -> bool {
        matches!(self,
            ContextType::Conversation |
            ContextType::Tree |
            ContextType::Todo |
            ContextType::Memory |
            ContextType::Overview
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
        }
    }

    /// Returns true if this context type uses cached_content from background loading
    pub fn needs_cache(&self) -> bool {
        matches!(self,
            ContextType::File |
            ContextType::Tree |
            ContextType::Glob |
            ContextType::Grep |
            ContextType::Tmux
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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
    /// Tool definitions with enabled state
    #[serde(default = "default_tools")]
    pub tools: Vec<ToolDefinition>,
    /// PID of the process that owns this state (for preventing concurrent instances)
    #[serde(default)]
    pub owner_pid: Option<u32>,
}

fn default_tools() -> Vec<ToolDefinition> {
    get_all_tool_definitions()
}

fn default_one() -> usize {
    1
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

        // Find first available starting from 6 (P1-P5 are fixed defaults)
        let id = (6..).find(|n| !used_ids.contains(n)).unwrap_or(6);
        format!("P{}", id)
    }
}
