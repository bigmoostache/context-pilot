use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::config::SidebarMode;
use super::context::{ContextElement, ContextType};
use super::message::Message;
use crate::cast::SafeCast;
use crate::llm_types::ModelInfo;
use crate::tools::ToolDefinition;
use crate::ui::render_cache::{FullContentCache, InputRenderCache, MessageRenderCache};

// =============================================================================
// Runtime State
// =============================================================================

/// Type alias for the syntax highlighting callback function.
/// Takes (`file_path`, content) and returns highlighted spans per line: Vec<Vec<(Color, String)>>
pub type HighlightFn = fn(&str, &str) -> std::sync::Arc<Vec<Vec<(ratatui::style::Color, String)>>>;

/// Runtime state (messages loaded in memory)
#[expect(clippy::struct_excessive_bools, reason = "Runtime state legitimately needs many boolean flags")]
pub struct State {
    /// Active context panels (dynamic + fixed), ordered by recency for LLM injection.
    pub context: Vec<ContextElement>,
    /// Conversation messages (user, assistant, `tool_call`, `tool_result`).
    pub messages: Vec<Message>,
    /// Current user input text in the editor.
    pub input: String,
    /// Cursor position in input (byte index)
    pub input_cursor: usize,
    /// Paste buffers: stored content for inline paste placeholders
    pub paste_buffers: Vec<String>,
    /// Labels for paste buffers: None = paste, Some(name) = command
    pub paste_buffer_labels: Vec<Option<String>>,
    /// Index of the currently selected context panel in the sidebar.
    pub selected_context: usize,
    /// Whether a stream is actively receiving tokens from the LLM.
    pub is_streaming: bool,
    /// Whether the system is currently executing tool calls (between stream ticks).
    /// True when tools are being dispatched, panels loading, console waits, or sleep timers.
    /// The LLM is not receiving tokens during this phase.
    pub is_tooling: bool,
    /// Stop reason from last completed stream (e.g., "`end_turn`", "`max_tokens`", "`tool_use`")
    pub last_stop_reason: Option<String>,
    /// Vertical scroll offset in the conversation view (fractional lines).
    pub scroll_offset: f32,
    /// Whether the user has manually scrolled (disables auto-scroll to bottom).
    pub user_scrolled: bool,
    /// Scroll acceleration (increases when holding scroll keys)
    pub scroll_accel: f32,
    /// Maximum scroll offset (set by UI based on content height)
    pub max_scroll: f32,
    /// Estimated tokens added during current streaming session (for correction when done)
    pub streaming_estimated_tokens: usize,
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
    pub llm_provider: crate::llm_types::LlmProvider,
    /// Active Anthropic model variant.
    pub anthropic_model: crate::llm_types::AnthropicModel,
    /// Active Grok model variant.
    pub grok_model: crate::llm_types::GrokModel,
    /// Active Groq model variant.
    pub groq_model: crate::llm_types::GroqModel,
    /// Active `DeepSeek` model variant.
    pub deepseek_model: crate::llm_types::DeepSeekModel,
    /// Whether config overlay is showing secondary model selection (Tab toggles)
    pub config_secondary_mode: bool,
    /// Secondary LLM provider (for reveries / sub-agents)
    pub secondary_provider: crate::llm_types::LlmProvider,
    /// Secondary Anthropic model variant.
    pub secondary_anthropic_model: crate::llm_types::AnthropicModel,
    /// Secondary Grok model variant.
    pub secondary_grok_model: crate::llm_types::GrokModel,
    /// Secondary Groq model variant.
    pub secondary_groq_model: crate::llm_types::GroqModel,
    /// Secondary `DeepSeek` model variant.
    pub secondary_deepseek_model: crate::llm_types::DeepSeekModel,
    /// Whether the reverie system is enabled (auto-trigger on threshold breach)
    pub reverie_enabled: bool,
    /// Sidebar display mode: Normal (full), Collapsed (icons only), Hidden
    pub sidebar_mode: SidebarMode,
    /// Active reverie sessions keyed by `agent_id` (e.g., "cleaner", "cartographer").
    /// Ephemeral — not persisted, discarded after each run.
    pub reveries: HashMap<String, super::reverie::ReverieState>,
    /// Accumulated `prompt_cache_hit_tokens` across all API calls (persisted)
    pub cache_hit_tokens: usize,
    /// Accumulated `prompt_cache_miss_tokens` across all API calls (persisted)
    pub cache_miss_tokens: usize,
    /// Accumulated output tokens across all API calls (persisted)
    pub total_output_tokens: usize,
    /// Current stream token accumulators (runtime-only, reset per user input)
    pub stream_cache_hit_tokens: usize,
    /// Cache misses in current stream.
    pub stream_cache_miss_tokens: usize,
    /// Output tokens in current stream.
    pub stream_output_tokens: usize,
    /// Last tick token accumulators (runtime-only, set per `StreamDone`)
    pub tick_cache_hit_tokens: usize,
    /// Cache misses in last completed tick.
    pub tick_cache_miss_tokens: usize,
    /// Output tokens in last completed tick.
    pub tick_output_tokens: usize,
    /// Cleaning threshold (0.0 - 1.0), triggers auto-cleaning when exceeded
    pub cleaning_threshold: f32,
    /// Cleaning target as proportion of threshold (0.0 - 1.0)
    pub cleaning_target_proportion: f32,
    /// Context budget in tokens (None = use model's full context window)
    pub context_budget: Option<usize>,

    /// Whether an API check is in progress
    pub api_check_in_progress: bool,
    /// Result of the last API check
    pub api_check_result: Option<crate::llm_types::ApiCheckResult>,
    /// Current API retry count (reset on success)
    pub api_retry_count: u32,
    /// Guard rail block reason (set when spine blocks, cleared when streaming starts)
    pub guard_rail_blocked: Option<String>,
    /// Reload pending (set by `system_reload`, triggers reload after tool result saved)
    pub reload_pending: bool,
    /// Waiting for file panels to load before continuing stream
    pub waiting_for_panels: bool,
    /// Previous panel hash list for cache cost tracking
    pub previous_panel_hash_list: Vec<String>,
    /// Sleep timer: tool pipeline waits until this timestamp (ms) before proceeding
    pub tool_sleep_until_ms: u64,

    // === Render Cache (runtime-only) ===
    /// Last viewport width used for render cache invalidation.
    pub last_viewport_width: u16,
    /// Cached rendered lines per message ID
    pub message_cache: HashMap<String, MessageRenderCache>,
    /// Cached rendered lines for input area
    pub input_cache: Option<InputRenderCache>,
    /// Full content cache (entire conversation output)
    pub full_content_cache: Option<FullContentCache>,

    // === Callback hooks (set by binary, used by extracted module crates) ===
    /// Syntax highlighting function (provided by binary's highlight module).
    /// Takes `(file_path, content)` and returns highlighted spans per line.
    pub highlight_fn: Option<HighlightFn>,

    // === Module extension data (TypeMap pattern) ===
    /// Module-owned state stored by `TypeId`. Each module registers its own state struct
    /// at startup via `Module::init_state()`. Accessed via `get_ext::<T>()` / `get_ext_mut::<T>()`.
    pub module_data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            // NOTE: context and tools are initialized empty here.
            // The binary populates them via the module registry during init.
            context: vec![],
            messages: vec![],
            input: String::new(),
            input_cursor: 0,
            paste_buffers: vec![],
            paste_buffer_labels: vec![],
            selected_context: 0,
            is_streaming: false,
            is_tooling: false,
            last_stop_reason: None,
            scroll_offset: 0.0,
            user_scrolled: false,
            scroll_accel: 1.0,
            max_scroll: 0.0,
            streaming_estimated_tokens: 0,
            next_user_id: 1,
            next_assistant_id: 1,
            next_tool_id: 1,
            next_result_id: 1,
            global_next_uid: 1,
            active_modules: std::collections::HashSet::new(),
            tools: vec![],
            dirty: true,
            spinner_frame: 0,
            dev_mode: false,
            perf_enabled: false,
            config_view: false,
            config_selected_bar: 0,
            active_theme: crate::config::DEFAULT_THEME.to_string(),
            llm_provider: crate::llm_types::LlmProvider::default(),
            anthropic_model: crate::llm_types::AnthropicModel::default(),
            grok_model: crate::llm_types::GrokModel::default(),
            groq_model: crate::llm_types::GroqModel::default(),
            deepseek_model: crate::llm_types::DeepSeekModel::default(),
            config_secondary_mode: false,
            secondary_provider: crate::llm_types::LlmProvider::Anthropic,
            secondary_anthropic_model: crate::llm_types::AnthropicModel::ClaudeHaiku45,
            secondary_grok_model: crate::llm_types::GrokModel::default(),
            secondary_groq_model: crate::llm_types::GroqModel::default(),
            secondary_deepseek_model: crate::llm_types::DeepSeekModel::default(),
            reverie_enabled: true,
            sidebar_mode: SidebarMode::Normal,
            reveries: HashMap::new(),
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
            context_budget: None,
            api_check_in_progress: false,
            api_check_result: None,
            api_retry_count: 0,
            guard_rail_blocked: None,
            reload_pending: false,
            waiting_for_panels: false,
            previous_panel_hash_list: vec![],
            tool_sleep_until_ms: 0,
            last_viewport_width: 0,
            message_cache: HashMap::new(),
            input_cache: None,
            full_content_cache: None,
            highlight_fn: None,
            module_data: HashMap::new(),
        }
    }
}

impl State {
    // === Module extension data (TypeMap) ===

    /// Get a reference to module-owned state by type.
    #[must_use]
    pub fn get_ext<T: 'static + Send + Sync>(&self) -> Option<&T> {
        self.module_data.get(&TypeId::of::<T>()).and_then(|v| v.downcast_ref())
    }

    /// Get a mutable reference to module-owned state by type.
    pub fn get_ext_mut<T: 'static + Send + Sync>(&mut self) -> Option<&mut T> {
        self.module_data.get_mut(&TypeId::of::<T>()).and_then(|v| v.downcast_mut())
    }

    /// Set module-owned state by type. Replaces any existing value of this type.
    pub fn set_ext<T: 'static + Send + Sync>(&mut self, val: T) {
        drop(self.module_data.insert(TypeId::of::<T>(), Box::new(val)));
    }

    /// Update the `last_refresh_ms` timestamp for a panel by its context type
    #[expect(
        clippy::needless_pass_by_value,
        reason = "ContextType is a lightweight string wrapper — pass by value is idiomatic"
    )]
    pub fn touch_panel(&mut self, context_type: ContextType) {
        if let Some(ctx) = self.context.iter_mut().find(|c| c.context_type == context_type) {
            ctx.last_refresh_ms = crate::panels::now_ms();
            ctx.cache_deprecated = true;
        }
        self.dirty = true;
    }

    /// Find the first available context ID (fills gaps instead of always incrementing)
    #[must_use]
    pub fn next_available_context_id(&self) -> String {
        let used_ids: std::collections::HashSet<usize> =
            self.context.iter().filter_map(|c| c.id.strip_prefix('P').and_then(|n| n.parse().ok())).collect();
        let id = (9..).find(|n| !used_ids.contains(n)).unwrap_or(9);
        format!("P{id}")
    }

    /// Get the API model string for the current provider/model selection
    #[must_use]
    pub fn current_model(&self) -> String {
        use crate::llm_types::LlmProvider;
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.api_name().to_string()
            }
            LlmProvider::Grok => self.grok_model.api_name().to_string(),
            LlmProvider::Groq => self.groq_model.api_name().to_string(),
            LlmProvider::DeepSeek => self.deepseek_model.api_name().to_string(),
        }
    }

    /// Get the max output tokens for the current provider/model selection
    #[must_use]
    pub fn current_max_output_tokens(&self) -> u32 {
        use crate::llm_types::LlmProvider;
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.max_output_tokens()
            }
            LlmProvider::Grok => self.grok_model.max_output_tokens(),
            LlmProvider::Groq => self.groq_model.max_output_tokens(),
            LlmProvider::DeepSeek => self.deepseek_model.max_output_tokens(),
        }
    }

    /// Get the max output tokens for the secondary provider/model selection
    #[must_use]
    pub fn secondary_max_output_tokens(&self) -> u32 {
        use crate::llm_types::LlmProvider;
        match self.secondary_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.secondary_anthropic_model.max_output_tokens()
            }
            LlmProvider::Grok => self.secondary_grok_model.max_output_tokens(),
            LlmProvider::Groq => self.secondary_groq_model.max_output_tokens(),
            LlmProvider::DeepSeek => self.secondary_deepseek_model.max_output_tokens(),
        }
    }

    /// Get the cleaning target as absolute proportion (threshold * `target_proportion`)
    #[must_use]
    pub fn cleaning_target(&self) -> f32 {
        self.cleaning_threshold * self.cleaning_target_proportion
    }

    /// Get the current model's context window
    #[must_use]
    pub fn model_context_window(&self) -> usize {
        use crate::llm_types::LlmProvider;
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.context_window()
            }
            LlmProvider::Grok => self.grok_model.context_window(),
            LlmProvider::Groq => self.groq_model.context_window(),
            LlmProvider::DeepSeek => self.deepseek_model.context_window(),
        }
    }

    /// Get effective context budget (custom or model's full context)
    #[must_use]
    pub fn effective_context_budget(&self) -> usize {
        self.context_budget.unwrap_or_else(|| self.model_context_window())
    }

    /// Get cleaning threshold in tokens
    #[must_use]
    pub fn cleaning_threshold_tokens(&self) -> usize {
        (self.effective_context_budget().to_f32() * self.cleaning_threshold).to_usize()
    }

    /// Get cleaning target in tokens
    #[must_use]
    pub fn cleaning_target_tokens(&self) -> usize {
        (self.effective_context_budget().to_f32() * self.cleaning_target()).to_usize()
    }

    /// Get cache hit price per million tokens for the current model
    #[must_use]
    pub fn cache_hit_price_per_mtok(&self) -> f32 {
        use crate::llm_types::LlmProvider;
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.cache_hit_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.cache_hit_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.cache_hit_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.cache_hit_price_per_mtok(),
        }
    }

    /// Get cache miss price per million tokens for the current model
    #[must_use]
    pub fn cache_miss_price_per_mtok(&self) -> f32 {
        use crate::llm_types::LlmProvider;
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.cache_miss_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.cache_miss_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.cache_miss_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.cache_miss_price_per_mtok(),
        }
    }

    /// Get output price per million tokens for the current model
    #[must_use]
    pub fn output_price_per_mtok(&self) -> f32 {
        use crate::llm_types::LlmProvider;
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.output_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.output_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.output_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.output_price_per_mtok(),
        }
    }

    /// Calculate cost in USD for a given token count and price per `MTok`
    #[must_use]
    pub fn token_cost(tokens: usize, price_per_mtok: f32) -> f64 {
        tokens.to_f64() * price_per_mtok.to_f64() / 1_000_000.0
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

    /// Create a user message and add it to the conversation.
    /// NOTE: Caller is responsible for persistence (`save_message`).
    /// Returns the index into self.messages.
    pub fn push_user_message(&mut self, content: String) -> usize {
        let token_count = super::estimate_tokens(&content);
        let (id, uid) = self.alloc_user_ids();
        let msg = Message::new_user(id, uid, content, token_count);

        if let Some(ctx) = self.context.iter_mut().find(|c| c.context_type == ContextType::CONVERSATION) {
            ctx.token_count += token_count;
            ctx.last_refresh_ms = crate::panels::now_ms();
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

    /// Prepare state for a new stream: set `is_streaming`, clear stop reason, reset tick counters.
    pub fn begin_streaming(&mut self) {
        self.is_streaming = true;
        self.last_stop_reason = None;
        self.streaming_estimated_tokens = 0;
        self.tick_cache_hit_tokens = 0;
        self.tick_cache_miss_tokens = 0;
        self.tick_output_tokens = 0;
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("context_len", &self.context.len())
            .field("messages_len", &self.messages.len())
            .field("is_streaming", &self.is_streaming)
            .field("module_data_keys", &self.module_data.len())
            .finish_non_exhaustive()
    }
}
