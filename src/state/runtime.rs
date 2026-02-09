use std::collections::HashMap;

use super::context::{ContextElement, ContextType};
use super::message::Message;
use super::render_cache::{MessageRenderCache, InputRenderCache, FullContentCache};

// Re-import module-owned types used in State fields
use crate::llms::ModelInfo;
use crate::modules::todo::types::TodoItem;
use crate::modules::memory::types::MemoryItem;
use crate::modules::prompt::types::SystemItem;
use crate::modules::scratchpad::types::ScratchpadCell;
use crate::modules::tree::types::{TreeFileDescription, DEFAULT_TREE_FILTER};
use crate::modules::git::types::GitFileChange;
use crate::tool_defs::ToolDefinition;

/// Runtime state (messages loaded in memory)
pub struct State {
    pub context: Vec<ContextElement>,
    pub messages: Vec<Message>,
    pub input: String,
    /// Cursor position in input (byte index)
    pub input_cursor: usize,
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
    /// Hash of last git status --porcelain output (for change detection)
    pub git_status_hash: Option<String>,
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
            git_status_hash: None,
            git_show_logs: false,
            git_log_args: None,
            git_log_content: None,
            git_diff_base: None,
            github_token: None,
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
}
