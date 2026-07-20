use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::context::{Entry, Kind};
use super::data::TickTelemetry;
use super::data::config::ViewMode;
use super::data::message::Message;
use super::flags::{HighlightIrFn, StatusBools, StreamPhase, StreamingTool};
use crate::config::llm_types::LlmProvider;
use crate::panels::ContextItem;
use crate::tools::ToolDefinition;
use crate::ui::render_cache::{FullCache, InputCache, MessageCache};

/// Ephemeral reverie sub-agent state (context optimizer, cartographer).
pub mod reverie;

// Runtime State

/// Runtime state (messages loaded in memory)
#[non_exhaustive]
pub struct State {
    /// Active context panels (dynamic + fixed), ordered by recency for LLM injection.
    pub context: Vec<Entry>,
    /// Conversation messages (user, assistant, `tool_call`, `tool_result`).
    pub messages: Vec<Message>,
    /// Current user input text in the editor.
    pub input: String,
    /// Cursor position in input (byte index)
    pub input_cursor: usize,
    /// Selection anchor (byte index). When set, text between anchor and cursor is selected.
    pub input_selection_anchor: Option<usize>,
    /// Paste buffers: stored content for inline paste placeholders
    pub paste_buffers: Vec<String>,
    /// Labels for paste buffers: None = paste, Some(name) = command
    pub paste_buffer_labels: Vec<Option<String>>,
    /// Index of the currently selected context panel in the sidebar.
    pub selected_context: usize,
    /// Boolean status flags, organized by domain.
    pub flags: StatusBools,
    /// Tool call currently being streamed (advisory, for UI rendering).
    pub streaming_tool: Option<StreamingTool>,
    /// Selected bar in config view (0=budget, 1=threshold, 2=target)
    pub config_selected_bar: usize,
    /// Stop reason from last completed stream (e.g., "`end_turn`", "`max_tokens`", "`tool_use`")
    pub last_stop_reason: Option<String>,
    /// Vertical scroll offset in the conversation view (fractional lines).
    pub scroll_offset: f32,
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
    /// Active theme ID (dnd, modern, futuristic, forest, sea, space)
    pub active_theme: String,
    /// Selected LLM provider
    pub llm_provider: LlmProvider,
    /// Active Anthropic model variant.
    pub anthropic_model: crate::config::models::AnthropicModel,
    /// Active Grok model variant.
    pub grok_model: crate::config::models::GrokModel,
    /// Active Groq model variant.
    pub groq_model: crate::config::models::GroqModel,
    /// Active `DeepSeek` model variant.
    pub deepseek_model: crate::config::models::DeepSeekModel,
    /// Active `MiniMax` model variant.
    pub minimax_model: crate::config::models::MiniMaxModel,
    /// Active Claude Code V2 model variant.
    pub claude_code_v2_model: crate::config::models::ClaudeCodeV2Model,
    /// View mode: Normal (full sidebar), Collapsed (icons), Hidden, Threads
    pub view_mode: ViewMode,
    /// Active reverie sessions keyed by `agent_id` (e.g., "cleaner", "cartographer").
    /// Ephemeral — not persisted, discarded after each run.
    pub reveries: HashMap<String, reverie::Session>,
    /// Accumulated `prompt_cache_hit_tokens` across all API calls (persisted)
    pub cache_hit_tokens: usize,
    /// Accumulated `prompt_cache_miss_tokens` across all API calls (persisted)
    pub cache_miss_tokens: usize,
    /// Accumulated output tokens across all API calls (persisted)
    pub total_output_tokens: usize,
    /// Accumulated uncached input tokens (after last cache breakpoint, billed at base price).
    /// Subset of `cache_miss_tokens` — tracked separately for sidebar display.
    pub uncached_input_tokens: usize,
    /// Current stream token accumulators (runtime-only, reset per user input)
    pub stream_cache_hit_tokens: usize,
    /// Cache misses in current stream.
    pub stream_cache_miss_tokens: usize,
    /// Output tokens in current stream.
    pub stream_output_tokens: usize,
    /// Uncached input tokens in current stream.
    pub stream_uncached_input_tokens: usize,
    /// Last tick token accumulators (runtime-only, set per `StreamDone`)
    pub tick_cache_hit_tokens: usize,
    /// Cache misses in last completed tick.
    pub tick_cache_miss_tokens: usize,
    /// Output tokens in last completed tick.
    pub tick_output_tokens: usize,
    /// Uncached input tokens in last completed tick.
    pub tick_uncached_input_tokens: usize,
    /// Cleaning threshold (0.0 - 1.0), triggers auto-cleaning when exceeded
    pub cleaning_threshold: f32,
    /// Context budget in tokens (None = use model's full context window)
    pub context_budget: Option<usize>,

    /// Accumulated cost in USD, frozen at consumption-time pricing.
    ///
    /// Unlike token counts (which are model-agnostic), cost is computed once per
    /// stream using the price active at that moment, then accumulated. Switching
    /// model afterwards does NOT retroactively rewrite these — past spend stays put.
    /// Cache-hit / cache-miss / output legs are tracked separately for the sidebar.
    pub cost_hit_usd: f64,
    /// Accumulated cache-miss cost in USD (frozen at consumption-time pricing).
    pub cost_miss_usd: f64,
    /// Accumulated output cost in USD (frozen at consumption-time pricing).
    pub cost_output_usd: f64,
    /// Current-stream cache-hit cost in USD (reset per user input).
    pub stream_cost_hit_usd: f64,
    /// Current-stream cache-miss cost in USD (reset per user input).
    pub stream_cost_miss_usd: f64,
    /// Current-stream output cost in USD (reset per user input).
    pub stream_cost_output_usd: f64,
    /// Last-tick cache-hit cost in USD (set per `StreamDone`).
    pub tick_cost_hit_usd: f64,
    /// Last-tick cache-miss cost in USD (set per `StreamDone`).
    pub tick_cost_miss_usd: f64,
    /// Last-tick output cost in USD (set per `StreamDone`).
    pub tick_cost_output_usd: f64,

    /// Result of the last API check
    pub api_check_result: Option<crate::config::llm_types::ApiCheckResult>,
    /// Current API retry count (reset on success)
    pub api_retry_count: u32,
    /// Guard rail block reason (set when spine blocks, cleared when streaming starts)
    pub guard_rail_blocked: Option<String>,
    /// Previous panel hash list for cache cost tracking
    pub previous_panel_hash_list: Vec<String>,
    /// Saved panel ID order from last emitted tick (for queue freeze stability)
    pub previous_panel_order: Vec<String>,
    /// Panel ID → context type from last emitted tick (for disappearance detection).
    pub previous_panel_id_types: Vec<(String, String)>,
    /// Panel IDs that carried a cache breakpoint on the last emitted tick, in
    /// prompt order (the BP→panel mapping recorded by the build path).
    ///
    /// Consumed by the freeze pass to widen the "free to update" region back to
    /// the last alive breakpoint before the culprit: panels between that
    /// breakpoint and the culprit are already billed fresh this turn, so
    /// refreshing them costs nothing. Runtime-only (empty on cold start ⇒ the
    /// freeze pass falls back to the culprit-anchored region).
    pub previous_breakpoint_panel_ids: Vec<String>,
    /// Full snapshot of panel `ContextItem`s from the last unfrozen tick.
    ///
    /// During tempo/queue freeze, this snapshot is replayed verbatim — guaranteeing
    /// byte-identical panel content and eliminating cache breaks from panel
    /// disappearance, appearance, or missing emitted snapshots.
    /// Not persisted across reloads (runtime-only).
    pub frozen_context_snapshot: Option<Vec<ContextItem>>,
    /// Sleep timer: tool pipeline waits until this timestamp (ms) before proceeding
    pub tool_sleep_until_ms: u64,
    /// Cache optimization engine: tracks accumulated hashes and breakpoint timestamps
    /// for intelligent Anthropic prompt cache breakpoint placement.
    /// Serialized through `WorkerState` modules for reload survival.
    pub cache_engine_json: Option<String>,
    /// Tempo flag: `true` means "nothing meaningful changed — freeze everything next tick."
    ///
    /// Set to `true` at the start of each tick. Any tool execution breaks it (sets `false`)
    /// unless the tool explicitly opts out via `ToolResult::preserves_tempo`. When the next
    /// `prepare_stream_context()` runs with `tempo == true`, ALL panels freeze unconditionally.
    pub tempo: bool,
    /// Pre-tick telemetry for cost-tracking TSV (populated at stream start, consumed at stream end).
    pub tick_telemetry: Option<TickTelemetry>,
    /// Number of alive (non-pruned) breakpoints at last tick — for sidebar display only.
    pub tick_alive_breakpoints: usize,
    /// Per-mille positions (0–1000) of alive BPs within the prompt, sorted.
    /// For the sidebar gauge showing WHERE breakpoints sit in the prompt.
    pub tick_alive_bp_positions: Vec<u16>,

    // === Render Cache (runtime-only) ===
    /// Last viewport width used for render cache invalidation.
    pub last_viewport_width: u16,
    /// Cached rendered lines per message ID
    pub message_cache: HashMap<String, MessageCache>,
    /// Cached rendered lines for input area
    pub input_cache: Option<InputCache>,
    /// Full content cache (entire conversation output)
    pub full_content_cache: Option<FullCache>,

    // === Callback hooks (set by binary, used by extracted module crates) ===
    /// IR-aware syntax highlighting (RGB colour spans for the IR pipeline).
    /// Takes `(file_path, content)` and returns `cp_render::Span` per line.
    pub highlight_ir_fn: Option<HighlightIrFn>,

    // === Module extension data (TypeMap pattern) ===
    /// Module-owned state stored by `TypeId`. Each module registers its own state struct
    /// at startup via `Module::init_state()`. Accessed via `get_ext::<T>()` / `get_ext_mut::<T>()`.
    pub module_data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

/// `Default` for `State` (extracted for the 500-line cap).
mod default;

impl State {
    // === Boot builder (cross-crate reconstruction from persisted state) ===

    /// Set the loaded context panels (builder).
    #[must_use]
    pub fn with_context(mut self, context: Vec<Entry>) -> Self {
        self.context = context;
        self
    }

    /// Set the loaded conversation messages (builder).
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Set the selected-panel index (builder).
    #[must_use]
    pub const fn with_selected_context(mut self, idx: usize) -> Self {
        self.selected_context = idx;
        self
    }

    /// Set the four message-ID counters as `(user, assistant, tool, result)` (builder).
    #[must_use]
    pub const fn with_id_counters(mut self, counters: (usize, usize, usize, usize)) -> Self {
        let (user, assistant, tool, result) = counters;
        self.next_user_id = user;
        self.next_assistant_id = assistant;
        self.next_tool_id = tool;
        self.next_result_id = result;
        self
    }

    /// Set the draft input text and cursor byte-offset (builder).
    #[must_use]
    pub fn with_draft(mut self, input: String, cursor: usize) -> Self {
        self.input = input;
        self.input_cursor = cursor;
        self
    }

    /// Set the view mode (builder).
    #[must_use]
    pub const fn with_view_mode(mut self, view_mode: ViewMode) -> Self {
        self.view_mode = view_mode;
        self
    }

    /// Set the active theme ID (builder).
    #[must_use]
    pub fn with_active_theme(mut self, theme: String) -> Self {
        self.active_theme = theme;
        self
    }

    /// Set the persisted cache-engine JSON blob (builder).
    #[must_use]
    pub fn with_cache_engine_json(mut self, json: Option<String>) -> Self {
        self.cache_engine_json = json;
        self
    }

    // === Module extension data (TypeMap) ===

    /// Get a reference to module-owned state by type.
    #[must_use]
    pub fn get_ext<T>(&self) -> Option<&T>
    where
        T: 'static + Send + Sync,
    {
        self.module_data.get(&TypeId::of::<T>()).and_then(|v| v.downcast_ref())
    }

    /// Get a mutable reference to module-owned state by type.
    pub fn get_ext_mut<T>(&mut self) -> Option<&mut T>
    where
        T: 'static + Send + Sync,
    {
        self.module_data.get_mut(&TypeId::of::<T>()).and_then(|v| v.downcast_mut())
    }

    /// Get module state by type, panicking if not initialized.
    ///
    /// Prefer this over `get_ext().expect()` — the panic lives in
    /// [`invariant_panic`](crate::config::invariant_panic) once,
    /// so callers don't need `expect(clippy::expect_used)`.
    ///
    /// # Panics
    ///
    /// Panics if module state `T` was never registered via [`set_ext`](Self::set_ext).
    #[must_use]
    pub fn ext<T>(&self) -> &T
    where
        T: 'static + Send + Sync,
    {
        self.get_ext::<T>().unwrap_or_else(|| {
            crate::config::invariant_panic("module state not initialized \u{2014} was init_state() called?")
        })
    }

    /// Get mutable module state by type, panicking if not initialized.
    ///
    /// # Panics
    ///
    /// Panics if module state `T` was never registered via [`set_ext`](Self::set_ext).
    pub fn ext_mut<T>(&mut self) -> &mut T
    where
        T: 'static + Send + Sync,
    {
        self.get_ext_mut::<T>().unwrap_or_else(|| {
            crate::config::invariant_panic("module state not initialized \u{2014} was init_state() called?")
        })
    }

    /// Set module-owned state by type. Replaces any existing value of this type.
    pub fn set_ext<T>(&mut self, val: T)
    where
        T: 'static + Send + Sync,
    {
        drop(self.module_data.insert(TypeId::of::<T>(), Box::new(val)));
    }

    /// Update the `last_refresh_ms` timestamp for a panel by its context type.
    pub fn touch_panel(&mut self, context_type: &str) {
        if let Some(ctx) = self.context.iter_mut().find(|c| c.context_type.as_str() == context_type) {
            ctx.last_refresh_ms = crate::panels::now_ms();
            ctx.cache_deprecated = true;
        }
        self.flags.ui.dirty = true;
    }

    /// Find the first available context ID (fills gaps instead of always incrementing)
    #[must_use]
    pub fn next_available_context_id(&self) -> String {
        let used_ids: std::collections::HashSet<usize> = self
            .context
            .iter()
            .filter_map(|c| {
                let n = c.id.strip_prefix('P')?;
                n.parse().ok()
            })
            .collect();
        let id = (9..10_000).find(|n| !used_ids.contains(n)).unwrap_or(9);
        format!("P{id}")
    }

    // === Message creation ===

    /// Allocate the next user message ID and UID, returning (id, uid).
    pub fn alloc_user_ids(&mut self) -> (String, String) {
        let id = format!("U{}", self.next_user_id);
        let uid = format!("UID_{}_U", self.global_next_uid);
        self.next_user_id = self.next_user_id.saturating_add(1);
        self.global_next_uid = self.global_next_uid.saturating_add(1);
        (id, uid)
    }

    /// Allocate the next assistant message ID and UID, returning (id, uid).
    pub fn alloc_assistant_ids(&mut self) -> (String, String) {
        let id = format!("A{}", self.next_assistant_id);
        let uid = format!("UID_{}_A", self.global_next_uid);
        self.next_assistant_id = self.next_assistant_id.saturating_add(1);
        self.global_next_uid = self.global_next_uid.saturating_add(1);
        (id, uid)
    }

    /// Create a user message and add it to the conversation.
    /// NOTE: Caller is responsible for persistence (`save_message`).
    /// Returns the index into self.messages.
    pub fn push_user_message(&mut self, content: String) -> usize {
        let token_count = super::context::estimate_tokens(&content);
        let (id, uid) = self.alloc_user_ids();
        let msg = Message::new_user(id, uid, content, token_count);

        if let Some(ctx) = self.context.iter_mut().find(|c| c.context_type.as_str() == Kind::CONVERSATION) {
            ctx.token_count = ctx.token_count.saturating_add(token_count);
            ctx.last_refresh_ms = crate::panels::now_ms();
        }

        self.messages.push(msg);
        self.messages.len().saturating_sub(1)
    }

    /// Create an empty assistant message for streaming into, add it, return its index.
    pub fn push_empty_assistant(&mut self) -> usize {
        let (id, uid) = self.alloc_assistant_ids();
        let msg = Message::new_assistant(id, uid);
        self.messages.push(msg);
        self.messages.len().saturating_sub(1)
    }

    /// Prepare state for a new stream: transition to [`StreamPhase::Receiving`],
    /// clear stop reason, reset tick counters.
    pub fn begin_streaming(&mut self) {
        self.flags.stream.phase.transition(StreamPhase::Receiving);
        self.last_stop_reason = None;
        self.streaming_estimated_tokens = 0;
        self.tick_cache_hit_tokens = 0;
        self.tick_cache_miss_tokens = 0;
        self.tick_output_tokens = 0;
        self.tick_uncached_input_tokens = 0;
        self.tick_cost_hit_usd = 0.0f64;
        self.tick_cost_miss_usd = 0.0f64;
        self.tick_cost_output_usd = 0.0f64;
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("context_len", &self.context.len())
            .field("messages_len", &self.messages.len())
            .field("stream_phase", &self.flags.stream.phase)
            .field("module_data_keys", &self.module_data.len())
            .finish_non_exhaustive()
    }
}
