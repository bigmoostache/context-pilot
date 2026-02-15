use std::collections::HashMap;

use super::context::{ContextElement, ContextType};
use super::message::Message;
use super::render_cache::{FullContentCache, InputRenderCache, MessageRenderCache};

// Re-import module-owned types used in State fields
use crate::llms::ModelInfo;
use crate::modules::git::types::GitFileChange;
use crate::modules::logs::types::LogEntry;
use crate::modules::memory::types::MemoryItem;
use crate::modules::prompt::types::{PromptItem, PromptType};
use crate::modules::scratchpad::types::ScratchpadCell;
use crate::modules::spine::types::{Notification, SpineConfig};
use crate::modules::todo::types::TodoItem;
use crate::modules::tree::types::{DEFAULT_TREE_FILTER, TreeFileDescription};
use crate::tool_defs::ToolDefinition;

/// Runtime state (messages loaded in memory)
pub struct State {
    pub context: Vec<ContextElement>,
    pub messages: Vec<Message>,
    pub input: String,
    /// Cursor position in input (byte index)
    pub input_cursor: usize,
    /// Paste buffers: stored content for inline paste placeholders
    pub paste_buffers: Vec<String>,
    /// Labels for paste buffers: None = paste, Some(name) = command expansion
    pub paste_buffer_labels: Vec<Option<String>>,
    pub selected_context: usize,
    pub is_streaming: bool,
    /// Stop reason from last completed stream (e.g., "end_turn", "max_tokens", "tool_use")
    pub last_stop_reason: Option<String>,
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
    /// IDs of memories whose full contents are shown (per-worker)
    pub open_memory_ids: Vec<String>,
    /// Agent prompt items
    pub agents: Vec<PromptItem>,
    /// Active agent ID (None = default)
    pub active_agent_id: Option<String>,
    /// Skill prompt items
    pub skills: Vec<PromptItem>,
    /// IDs of skills that have open panels
    pub loaded_skill_ids: Vec<String>,
    /// Command prompt items
    pub commands: Vec<PromptItem>,
    /// Preview in P8 Library panel: (PromptType, id)
    pub library_preview: Option<(PromptType, String)>,
    /// Scratchpad cells
    pub scratchpad_cells: Vec<ScratchpadCell>,
    /// Next scratchpad cell ID (C1, C2, ...)
    pub next_scratchpad_id: usize,
    /// Log entries (timestamped short notes)
    pub logs: Vec<LogEntry>,
    /// Next log entry ID (L1, L2, ...)
    pub next_log_id: usize,
    /// IDs of log summaries whose children are expanded (per-worker)
    pub open_log_ids: Vec<String>,
    /// Spine notifications
    pub notifications: Vec<Notification>,
    /// Next notification ID (N1, N2, ...)
    pub next_notification_id: usize,
    /// Spine module configuration (per-worker)
    pub spine_config: SpineConfig,
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
    /// Selected DeepSeek model
    pub deepseek_model: crate::llms::DeepSeekModel,
    /// Accumulated prompt_cache_hit_tokens across all API calls (persisted)
    pub cache_hit_tokens: usize,
    /// Accumulated prompt_cache_miss_tokens across all API calls (persisted)
    pub cache_miss_tokens: usize,
    /// Accumulated output tokens across all API calls (persisted)
    pub total_output_tokens: usize,
    /// Current stream accumulated cache hit tokens (runtime-only, reset per user input)
    pub stream_cache_hit_tokens: usize,
    /// Current stream accumulated cache miss tokens (runtime-only, reset per user input)
    pub stream_cache_miss_tokens: usize,
    /// Current stream accumulated output tokens (runtime-only, reset per user input)
    pub stream_output_tokens: usize,
    /// Last tick cache hit tokens (runtime-only, set per StreamDone)
    pub tick_cache_hit_tokens: usize,
    /// Last tick cache miss tokens (runtime-only, set per StreamDone)
    pub tick_cache_miss_tokens: usize,
    /// Last tick output tokens (runtime-only, set per StreamDone)
    pub tick_output_tokens: usize,
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
    /// Whether to show git log in Git panel
    pub git_show_logs: bool,
    /// Custom git log arguments (e.g., "-5 --oneline")
    pub git_log_args: Option<String>,
    /// Cached git log output
    pub git_log_content: Option<String>,
    /// Diff base ref for P6 (e.g., "HEAD~3", "main")
    pub git_diff_base: Option<String>,
    /// GitHub personal access token (from GITHUB_TOKEN env)
    pub github_token: Option<String>,
    /// Current API retry count (reset on success)
    pub api_retry_count: u32,
    /// Reload pending flag (set by system_reload tool, triggers reload after tool result is saved)
    pub reload_pending: bool,
    /// Waiting for file panels to load before continuing stream
    pub waiting_for_panels: bool,
    /// Previous panel hash list for cache cost tracking (ordered hashes from last tick)
    pub previous_panel_hash_list: Vec<String>,
    /// Sleep timer: if nonzero, tool pipeline should wait until this timestamp (ms) before proceeding
    pub tool_sleep_until_ms: u64,
    /// Whether to refresh tmux panels after tool_sleep_until_ms expires (set by send_keys, not by sleep)
    pub tool_sleep_needs_tmux_refresh: bool,

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

impl Default for State {
    fn default() -> Self {
        Self {
            context: crate::modules::all_fixed_panel_defaults()
                .iter()
                .enumerate()
                .map(|(i, (_, _, ct, name, cache_dep))| {
                    crate::modules::make_default_context_element(&format!("P{}", i), *ct, name, *cache_dep)
                })
                .collect(),
            messages: vec![],
            input: String::new(),
            input_cursor: 0,
            paste_buffers: vec![],
            paste_buffer_labels: vec![],
            selected_context: 0,
            is_streaming: false,
            last_stop_reason: None,
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
            open_memory_ids: vec![],
            agents: vec![],
            active_agent_id: None,
            skills: vec![],
            loaded_skill_ids: vec![],
            commands: vec![],
            library_preview: None,
            scratchpad_cells: vec![],
            next_scratchpad_id: 1,
            logs: vec![],
            next_log_id: 1,
            open_log_ids: vec![],
            notifications: vec![],
            next_notification_id: 1,
            spine_config: SpineConfig::default(),
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
            deepseek_model: crate::llms::DeepSeekModel::default(),
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            total_output_tokens: 0,
            stream_cache_hit_tokens: 0,
            stream_cache_miss_tokens: 0,
            stream_output_tokens: 0,
            tick_cache_hit_tokens: 0,
            tick_cache_miss_tokens: 0,
            tick_output_tokens: 0,
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
            git_show_logs: false,
            git_log_args: None,
            git_log_content: None,
            git_diff_base: None,
            github_token: None,
            // API retry
            api_retry_count: 0,
            reload_pending: false,
            waiting_for_panels: false,
            previous_panel_hash_list: vec![],
            tool_sleep_until_ms: 0,
            tool_sleep_needs_tmux_refresh: false,
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
            ctx.cache_deprecated = true;
        }
        self.dirty = true;
    }

    /// Find the first available context ID (fills gaps instead of always incrementing)
    pub fn next_available_context_id(&self) -> String {
        // Collect all existing numeric IDs
        let used_ids: std::collections::HashSet<usize> =
            self.context.iter().filter_map(|c| c.id.strip_prefix('P').and_then(|n| n.parse().ok())).collect();

        // Find first available starting from 9 (P0-P8 are fixed defaults)
        let id = (9..).find(|n| !used_ids.contains(n)).unwrap_or(9);
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
            crate::llms::LlmProvider::DeepSeek => self.deepseek_model.api_name().to_string(),
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
            crate::llms::LlmProvider::DeepSeek => self.deepseek_model.context_window(),
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

    /// Get cache hit price per million tokens for the current model
    pub fn cache_hit_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            crate::llms::LlmProvider::Anthropic | crate::llms::LlmProvider::ClaudeCode => {
                self.anthropic_model.cache_hit_price_per_mtok()
            }
            crate::llms::LlmProvider::Grok => self.grok_model.cache_hit_price_per_mtok(),
            crate::llms::LlmProvider::Groq => self.groq_model.cache_hit_price_per_mtok(),
            crate::llms::LlmProvider::DeepSeek => self.deepseek_model.cache_hit_price_per_mtok(),
        }
    }

    /// Get cache miss price per million tokens for the current model
    pub fn cache_miss_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            crate::llms::LlmProvider::Anthropic | crate::llms::LlmProvider::ClaudeCode => {
                self.anthropic_model.cache_miss_price_per_mtok()
            }
            crate::llms::LlmProvider::Grok => self.grok_model.cache_miss_price_per_mtok(),
            crate::llms::LlmProvider::Groq => self.groq_model.cache_miss_price_per_mtok(),
            crate::llms::LlmProvider::DeepSeek => self.deepseek_model.cache_miss_price_per_mtok(),
        }
    }

    /// Get output price per million tokens for the current model
    pub fn output_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            crate::llms::LlmProvider::Anthropic | crate::llms::LlmProvider::ClaudeCode => {
                self.anthropic_model.output_price_per_mtok()
            }
            crate::llms::LlmProvider::Grok => self.grok_model.output_price_per_mtok(),
            crate::llms::LlmProvider::Groq => self.groq_model.output_price_per_mtok(),
            crate::llms::LlmProvider::DeepSeek => self.deepseek_model.output_price_per_mtok(),
        }
    }

    /// Calculate cost in USD for a given token count and price per MTok
    pub fn token_cost(tokens: usize, price_per_mtok: f32) -> f64 {
        tokens as f64 * price_per_mtok as f64 / 1_000_000.0
    }

    // === Message Creation Helpers ===

    /// Allocate the next user message ID and UID, returning (id, uid).
    pub fn alloc_user_ids(&mut self) -> (String, String) {
        let id = format!("U{}", self.next_user_id);
        let uid = format!("UID_{}_U", self.global_next_uid);
        self.next_user_id += 1;
        self.global_next_uid += 1;
        (id, uid)
    }

    /// Allocate the next assistant message ID and UID, returning (id, uid).
    pub fn alloc_assistant_ids(&mut self) -> (String, String) {
        let id = format!("A{}", self.next_assistant_id);
        let uid = format!("UID_{}_A", self.global_next_uid);
        self.next_assistant_id += 1;
        self.global_next_uid += 1;
        (id, uid)
    }

    /// Create a user message, add it to the conversation, update conversation token count,
    /// and persist it. Returns the index into self.messages.
    pub fn push_user_message(&mut self, content: String) -> usize {
        let token_count = crate::state::estimate_tokens(&content);
        let (id, uid) = self.alloc_user_ids();
        let msg = Message::new_user(id, uid, content, token_count);
        crate::persistence::save_message(&msg);

        if let Some(ctx) = self.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
            ctx.token_count += token_count;
            ctx.last_refresh_ms = crate::core::panels::now_ms();
        }

        self.messages.push(msg);
        self.messages.len() - 1
    }

    /// Create an empty assistant message for streaming into, add it, return its index.
    pub fn push_empty_assistant(&mut self) -> usize {
        let (id, uid) = self.alloc_assistant_ids();
        let msg = Message::new_assistant(id, uid);
        self.messages.push(msg);
        self.messages.len() - 1
    }

    /// Prepare state for a new stream: set is_streaming, clear stop reason, reset tick counters.
    pub fn begin_streaming(&mut self) {
        self.is_streaming = true;
        self.last_stop_reason = None;
        self.streaming_estimated_tokens = 0;
        self.tick_cache_hit_tokens = 0;
        self.tick_cache_miss_tokens = 0;
        self.tick_output_tokens = 0;
    }

    // === Spine / Notification Helpers ===

    /// Create a new notification and add it to the notification list.
    /// Returns the notification ID.
    pub fn create_notification(
        &mut self,
        notification_type: crate::modules::spine::types::NotificationType,
        source: String,
        content: String,
    ) -> String {
        let id = format!("N{}", self.next_notification_id);
        self.next_notification_id += 1;
        let notification =
            crate::modules::spine::types::Notification::new(id.clone(), notification_type, source, content);
        self.notifications.push(notification);
        // Garbage-collect old processed notifications (cap at 100)
        self.gc_notifications(100);
        // Mark spine panel as needing refresh
        self.touch_panel(ContextType::Spine);
        id
    }

    /// Mark a notification as processed by ID. Returns true if found.
    pub fn mark_notification_processed(&mut self, id: &str) -> bool {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.processed = true;
            self.touch_panel(ContextType::Spine);
            true
        } else {
            false
        }
    }

    /// Get references to all unprocessed notifications
    pub fn unprocessed_notifications(&self) -> Vec<&crate::modules::spine::types::Notification> {
        self.notifications.iter().filter(|n| !n.processed).collect()
    }

    /// Check if there are any unprocessed notifications
    pub fn has_unprocessed_notifications(&self) -> bool {
        self.notifications.iter().any(|n| !n.processed)
    }

    /// Garbage-collect old processed notifications to prevent unbounded growth.
    /// Keeps all unprocessed notifications and the most recent processed ones,
    /// capping the total list at `max` entries.
    pub fn gc_notifications(&mut self, max: usize) {
        if self.notifications.len() <= max {
            return;
        }
        // Remove oldest processed notifications first (they're at the front)
        let excess = self.notifications.len() - max;
        let mut removed = 0usize;
        self.notifications.retain(|n| {
            if removed >= excess {
                return true;
            }
            if n.processed {
                removed += 1;
                return false;
            }
            true // Keep unprocessed
        });
        if removed > 0 {
            self.touch_panel(ContextType::Spine);
        }
    }

    /// Mark all "transparent" notifications (UserMessage, ReloadResume) as processed.
    /// Called when a new stream starts — the LLM sees them via rebuilt context.
    pub fn mark_user_message_notifications_processed(&mut self) {
        use crate::modules::spine::types::NotificationType;
        let mut changed = false;
        for n in &mut self.notifications {
            if !n.processed
                && matches!(n.notification_type, NotificationType::UserMessage | NotificationType::ReloadResume)
            {
                n.processed = true;
                changed = true;
            }
        }
        if changed {
            self.touch_panel(ContextType::Spine);
        }
    }

    // === Todo helpers for spine (avoid spine importing todo types directly) ===

    /// Check if there are any pending or in-progress todos
    pub fn has_incomplete_todos(&self) -> bool {
        use crate::modules::todo::types::TodoStatus;
        self.todos.iter().any(|t| matches!(t.status, TodoStatus::Pending | TodoStatus::InProgress))
    }

    /// Get a summary of incomplete todos for spine auto-continuation messages
    pub fn incomplete_todos_summary(&self) -> Vec<String> {
        use crate::modules::todo::types::TodoStatus;
        self.todos
            .iter()
            .filter(|t| matches!(t.status, TodoStatus::Pending | TodoStatus::InProgress))
            .map(|t| format!("[{}] {} — {}", t.id, t.status.icon(), t.name))
            .collect()
    }
}
