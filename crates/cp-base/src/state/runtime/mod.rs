use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::context::{Entry, Kind};
use super::data::config::ViewMode;
use super::data::message::Message;
use super::flags::{HighlightIrFn, StatusBools, StreamPhase};
use crate::config::llm::types::LlmProvider;
use crate::tools::ToolDefinition;

/// Ephemeral reverie sub-agent state (context optimizer, cartographer).
pub mod reverie;
/// Shared text-editing engine (buffer, cursor, selection, undo) for textareas.
pub mod textarea;

// Runtime State

/// Runtime application state.
///
/// `State` holds **only fleet-shared data** — things that are single-instance
/// across every thread: the active model/provider, the theme, the tool set, the
/// fleet-shared module `TypeMap`, the global UID counter, UI-global flags, and
/// the ephemeral reverie sessions.
///
/// **No conversation, no panels, no per-thread anything lives here as a field.**
/// Everything a thread owns (its messages, its panel/context set, its editor and
/// scroll state, its token/cost telemetry, its cache/freeze engine snapshots,
/// its per-thread module data, its stream phase) lives on a
/// [`ThreadRuntime`](bundle::ThreadRuntime) stored in
/// [`thread_store`](Self::thread_store), keyed by thread id. Code reaches the
/// executing thread's data explicitly through [`thread`](Self::thread) /
/// [`thread_mut`](Self::thread_mut); `State` itself holds only shared data.
pub struct State {
    /// Every thread's per-thread context (conversation, panels, editor/scroll,
    /// token/cost telemetry, cache/freeze snapshots, stream phase, per-thread
    /// module `TypeMap`), stored permanently by thread id, plus the id of the
    /// thread executing right now. `State` derefs to the executing thread's
    /// runtime; changing it moves no data.
    pub thread_store: threads::ThreadStore,

    /// Boolean status flags that are fleet-global (UI redraw, config overlay,
    /// reload lifecycle, module overlays). Per-thread stream/scroll state is on
    /// [`executing.stream`](bundle::ThreadRuntime::stream), reached via deref as
    /// `state.stream`.
    pub flags: StatusBools,
    /// Selected bar in config view (0=budget, 1=threshold, 2=target)
    pub config_selected_bar: usize,
    /// Global UID counter for all shared elements (messages, panels)
    pub global_next_uid: usize,
    /// Cleaning threshold (0.0–1.0) of the context budget that triggers
    /// auto-cleaning. Fleet-wide: the Ctrl+H setting applies to every thread.
    pub cleaning_threshold: f32,
    /// Context budget in tokens (`None` = model's full window). Fleet-wide.
    pub context_budget: Option<usize>,
    /// Tool definitions with enabled state
    pub tools: Vec<ToolDefinition>,
    /// Active module IDs
    pub active_modules: std::collections::HashSet<String>,
    /// Active theme ID (dnd, modern, futuristic, forest, sea, space)
    pub active_theme: String,
    /// Selected LLM provider
    pub llm_provider: LlmProvider,
    /// Active Anthropic model variant.
    pub anthropic_model: crate::config::llm::models::AnthropicModel,
    /// Active Grok model variant.
    pub grok_model: crate::config::llm::models::GrokModel,
    /// Active Groq model variant.
    pub groq_model: crate::config::llm::models::GroqModel,
    /// Active `DeepSeek` model variant.
    pub deepseek_model: crate::config::llm::models::DeepSeekModel,
    /// Active `MiniMax` model variant.
    pub minimax_model: crate::config::llm::models::MiniMaxModel,
    /// Active `OpenRouter` model variant.
    pub openrouter_model: crate::config::llm::openrouter_model::OpenRouterModel,
    /// Active Claude Code V2 model variant.
    pub claude_code_v2_model: crate::config::llm::models::ClaudeCodeV2Model,
    /// View mode: Normal (full sidebar), Collapsed (icons), Hidden, Threads
    pub view_mode: ViewMode,
    /// Active reverie sessions keyed by `agent_id` (e.g., "cleaner", "cartographer").
    /// Ephemeral — not persisted, discarded after each run.
    pub reveries: HashMap<String, reverie::Session>,
    /// Result of the last API check
    pub api_check_result: Option<crate::config::llm::types::ApiCheckResult>,

    // === Callback hooks (set by binary, used by extracted module crates) ===
    /// IR-aware syntax highlighting (RGB colour spans for the IR pipeline).
    /// Takes `(file_path, content)` and returns `cp_render::Span` per line.
    pub highlight_ir_fn: Option<HighlightIrFn>,

    // === Module extension data (fleet-shared half; per-thread half on executing) ===
    /// Fleet-shared module-owned state stored by `TypeId` (one instance across
    /// all threads — e.g. memory, logs, entities, the threads registry). The
    /// per-thread half lives on [`executing.thread_module_data`](bundle::ThreadRuntime::thread_module_data).
    ///
    /// A given `TypeId` lives in exactly ONE of the two maps, so the `get_ext`
    /// family searches both and `set_ext` updates whichever already holds the
    /// type; first-inserts are routed by [`init_is_global`](Self::init_is_global).
    pub shared_module_data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    /// Ambient scope for the *next* first-insert via [`set_ext`](Self::set_ext),
    /// set by the boot/init loops around `init_state` / `load_module_data`:
    /// `Some(true)` → [`shared_module_data`](Self::shared_module_data),
    /// `Some(false)` or `None` → the executing thread's per-thread map.
    /// Updates to already-registered types ignore this (they stay in place).
    pub init_is_global: Option<bool>,
}

/// Per-thread runtime bundle (`ThreadRuntime`).
pub mod bundle;
/// `Default` for `State` (extracted for the 500-line cap).
mod default;
/// Module extension-data accessors (`get_ext`/`ext`/`set_ext`/…), extracted for the cap.
mod ext;
/// Thread-runtime storage keyed by id + the executing-thread id (`ThreadStore`).
pub mod threads;

impl State {
    /// The thread executing right now: owner of every per-thread write (todos,
    /// scratchpad, tool traces, notifications). Never the human's focus — that
    /// is UI-only and can point at another thread while this one runs.
    /// `None` when no thread is placed (cold boot, focus cleared).
    #[must_use]
    pub fn executing_thread_id(&self) -> Option<&str> {
        self.thread_store.executing()
    }

    /// The executing thread's runtime (conversation, panels, stream, per-thread
    /// module data). The unbound runtime when no thread is executing.
    #[must_use]
    pub fn thread(&self) -> &bundle::ThreadRuntime {
        self.thread_store.current()
    }

    /// Mutable twin of [`thread`](Self::thread).
    pub fn thread_mut(&mut self) -> &mut bundle::ThreadRuntime {
        self.thread_store.current_mut()
    }

    // === Boot builder (cross-crate reconstruction from persisted state) ===

    /// Set the loaded context panels (builder).
    #[must_use]
    pub fn with_context(mut self, context: Vec<Entry>) -> Self {
        self.thread_mut().context = context;
        self
    }

    /// Set the loaded conversation messages (builder).
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.thread_mut().messages = messages;
        self
    }

    /// Set the selected-panel index (builder).
    #[must_use]
    pub fn with_selected_context(mut self, idx: usize) -> Self {
        self.thread_mut().selected_context = idx;
        self
    }

    /// Set the four message-ID counters as `(user, assistant, tool, result)` (builder).
    #[must_use]
    pub fn with_id_counters(mut self, counters: (usize, usize, usize, usize)) -> Self {
        let (user, assistant, tool, result) = counters;
        self.thread_mut().next_user_id = user;
        self.thread_mut().next_assistant_id = assistant;
        self.thread_mut().next_tool_id = tool;
        self.thread_mut().next_result_id = result;
        self
    }

    /// Set the draft input text and cursor byte-offset (builder).
    #[must_use]
    pub fn with_draft(mut self, input: String, cursor: usize) -> Self {
        self.thread_mut().composer.text = input;
        self.thread_mut().composer.cursor = cursor;
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
        self.thread_mut().cache_engine_json = json;
        self
    }

    // === Module extension data (TypeMap) ===
    // Accessors (get_ext/ext/set_ext/…) live in the `ext` sibling module.

    /// Update the `last_refresh_ms` timestamp for a panel by its context type.
    pub fn touch_panel(&mut self, context_type: &str) {
        if let Some(ctx) = self.thread_mut().context.iter_mut().find(|c| c.context_type.as_str() == context_type) {
            ctx.last_refresh_ms = crate::panels::now_ms();
            ctx.cache_deprecated = true;
        }
        self.flags.ui.dirty = true;
    }

    /// Find the first available context ID (fills gaps instead of always incrementing)
    #[must_use]
    pub fn next_available_context_id(&self) -> String {
        let used_ids: std::collections::HashSet<usize> = self
            .thread()
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
        let id = format!("U{}", self.thread().next_user_id);
        let uid = format!("UID_{}_U", self.global_next_uid);
        self.thread_mut().next_user_id = self.thread_mut().next_user_id.saturating_add(1);
        self.global_next_uid = self.global_next_uid.saturating_add(1);
        (id, uid)
    }

    /// Allocate the next assistant message ID and UID, returning (id, uid).
    pub fn alloc_assistant_ids(&mut self) -> (String, String) {
        let id = format!("A{}", self.thread().next_assistant_id);
        let uid = format!("UID_{}_A", self.global_next_uid);
        self.thread_mut().next_assistant_id = self.thread_mut().next_assistant_id.saturating_add(1);
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

        if let Some(ctx) = self.thread_mut().context.iter_mut().find(|c| c.context_type.as_str() == Kind::CONVERSATION)
        {
            ctx.token_count = ctx.token_count.saturating_add(token_count);
            ctx.last_refresh_ms = crate::panels::now_ms();
        }

        self.thread_mut().messages.push(msg);
        self.thread().messages.len().saturating_sub(1)
    }

    /// Remove all injected `/* Notification [...] */` user messages from the
    /// conversation (T736 aggregation: keep at most one notification message
    /// live at a time). Returns the count removed.
    ///
    /// These messages are standalone user text (never part of a `tool_use` /
    /// `tool_result` pairing), so removing them can never orphan a tool block.
    /// Orphaned message files left on disk are harmless: boot loads strictly
    /// from the persisted `message_uids` index, which is regenerated from this
    /// (stripped) message list on the next state save.
    pub fn strip_notification_messages(&mut self) -> usize {
        let before = self.thread().messages.len();
        self.thread_mut()
            .messages
            .retain(|m| !(m.role == "user" && m.content.trim_start().starts_with("/* Notification [")));
        before.saturating_sub(self.thread().messages.len())
    }

    /// Create an empty assistant message for streaming into, add it, return its index.
    pub fn push_empty_assistant(&mut self) -> usize {
        let (id, uid) = self.alloc_assistant_ids();
        let msg = Message::new_assistant(id, uid);
        self.thread_mut().messages.push(msg);
        self.thread().messages.len().saturating_sub(1)
    }

    /// Prepare state for a new stream: transition to [`StreamPhase::Receiving`],
    /// clear stop reason, reset tick counters.
    pub fn begin_streaming(&mut self) {
        self.thread_mut().stream.phase.transition(StreamPhase::Receiving);
        self.thread_mut().last_stop_reason = None;
        self.thread_mut().streaming_estimated_tokens = 0;
        self.thread_mut().tick_cache_hit_tokens = 0;
        self.thread_mut().tick_cache_miss_tokens = 0;
        self.thread_mut().tick_output_tokens = 0;
        self.thread_mut().tick_uncached_input_tokens = 0;
        self.thread_mut().tick_cost_hit_usd = 0.0f64;
        self.thread_mut().tick_cost_miss_usd = 0.0f64;
        self.thread_mut().tick_cost_output_usd = 0.0f64;
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("context_len", &self.thread().context.len())
            .field("messages_len", &self.thread().messages.len())
            .field("stream_phase", &self.thread().stream.phase)
            .field(
                "module_data_keys",
                &self.shared_module_data.len().saturating_add(self.thread().thread_module_data.len()),
            )
            .finish_non_exhaustive()
    }
}
