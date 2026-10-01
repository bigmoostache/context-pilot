//! Per-thread runtime bundle — the mutable context a single thread owns.
//!
//! The multi-thread model keeps [`State`] *flat*: the whole existing pipeline
//! (stream processing, tool execution, the cache/freeze engine) reads and writes
//! `state.messages` / `state.context` / `state.flags.stream` directly and must
//! stay untouched. Instead of threading a per-thread context parameter through
//! all of it, the loop keeps one thread *resident* in `State` and swaps a
//! different thread in/out around a single advancement step.
//!
//! [`ThreadRuntime`] is the carrier for that swap: it mirrors exactly the subset
//! of `State` fields that belong to one thread (conversation, editor/view state,
//! token+cost telemetry, the cache/freeze engine snapshots, per-thread module
//! data). [`ThreadRuntime::swap_with`] exchanges every one of those fields with a
//! `State` in O(1) via [`std::mem::swap`] — no clone, no pipeline change. Fields
//! NOT present here (tools, active modules, theme, provider/model, the shared
//! module `TypeMap`, `global_next_uid`, the highlight fn, reveries) are
//! fleet-shared and never move.
//!
//! This module is purely additive until the loop is wired (Phase C): nothing
//! calls [`swap_with`](ThreadRuntime::swap_with) yet, so behaviour at a single
//! resident thread is unchanged.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::State;
use crate::panels::ContextItem;
use crate::state::context::Entry;
use crate::state::data::TickTelemetry;
use crate::state::data::message::Message;
use crate::state::flags::{StreamState, StreamingTool};
use crate::ui::render_cache::{FullCache, InputCache, MessageCache};

/// The complete mutable context owned by one thread.
///
/// Every field mirrors a per-thread field of [`State`] (same name, same type);
/// [`swap_with`](Self::swap_with) exchanges them all with a `State`. The resident
/// thread's values live in `State`; every non-resident thread parks its values
/// here inside the fleet registry.
pub struct ThreadRuntime {
    // === Conversation + panels ===
    /// Active context panels (dynamic + fixed), ordered for LLM injection.
    pub context: Vec<Entry>,
    /// Conversation messages (user, assistant, `tool_call`, `tool_result`).
    pub messages: Vec<Message>,

    // === Editor / view state ===
    /// Current user input text in the editor.
    pub input: String,
    /// Cursor position in input (byte index).
    pub input_cursor: usize,
    /// Selection anchor (byte index); `Some` while a selection is active.
    pub input_selection_anchor: Option<usize>,
    /// Paste buffers: stored content for inline paste placeholders.
    pub paste_buffers: Vec<String>,
    /// Labels for paste buffers: `None` = paste, `Some(name)` = command.
    pub paste_buffer_labels: Vec<Option<String>>,
    /// Index of the currently selected context panel in the sidebar.
    pub selected_context: usize,
    /// Vertical scroll offset in the conversation view (fractional lines).
    pub scroll_offset: f32,
    /// Scroll acceleration (increases while holding scroll keys).
    pub scroll_accel: f32,
    /// Maximum scroll offset (set by the UI from content height).
    pub max_scroll: f32,

    // === Stream lifecycle ===
    /// Current [`StreamState`] (phase + scroll tracking) for this thread.
    pub stream: StreamState,
    /// Tool call currently being streamed (advisory, for UI rendering).
    pub streaming_tool: Option<StreamingTool>,
    /// Stop reason from the last completed stream (e.g. `"end_turn"`).
    pub last_stop_reason: Option<String>,
    /// Estimated tokens added during the current streaming session.
    pub streaming_estimated_tokens: usize,
    /// Whether this thread is waiting for file panels before continuing its stream.
    pub waiting_for_panels: bool,

    // === Per-conversation message-ID counters ===
    /// Next user message ID (U1, U2, …).
    pub next_user_id: usize,
    /// Next assistant message ID (A1, A2, …).
    pub next_assistant_id: usize,
    /// Next tool message ID (T1, T2, …).
    pub next_tool_id: usize,
    /// Next result message ID (R1, R2, …).
    pub next_result_id: usize,

    // === Token accumulators (per-thread budget) ===
    /// Accumulated `prompt_cache_hit_tokens` across API calls.
    pub cache_hit_tokens: usize,
    /// Accumulated `prompt_cache_miss_tokens` across API calls.
    pub cache_miss_tokens: usize,
    /// Accumulated output tokens across API calls.
    pub total_output_tokens: usize,
    /// Accumulated uncached input tokens (billed at base price).
    pub uncached_input_tokens: usize,
    /// Current-stream cache-hit tokens (reset per user input).
    pub stream_cache_hit_tokens: usize,
    /// Current-stream cache-miss tokens.
    pub stream_cache_miss_tokens: usize,
    /// Current-stream output tokens.
    pub stream_output_tokens: usize,
    /// Current-stream uncached input tokens.
    pub stream_uncached_input_tokens: usize,
    /// Last-tick cache-hit tokens (set per `StreamDone`).
    pub tick_cache_hit_tokens: usize,
    /// Last-tick cache-miss tokens.
    pub tick_cache_miss_tokens: usize,
    /// Last-tick output tokens.
    pub tick_output_tokens: usize,
    /// Last-tick uncached input tokens.
    pub tick_uncached_input_tokens: usize,

    // === Cost accumulators (USD, frozen at consumption-time pricing) ===
    /// Accumulated cache-hit cost in USD.
    pub cost_hit_usd: f64,
    /// Accumulated cache-miss cost in USD.
    pub cost_miss_usd: f64,
    /// Accumulated output cost in USD.
    pub cost_output_usd: f64,
    /// Current-stream cache-hit cost in USD.
    pub stream_cost_hit_usd: f64,
    /// Current-stream cache-miss cost in USD.
    pub stream_cost_miss_usd: f64,
    /// Current-stream output cost in USD.
    pub stream_cost_output_usd: f64,
    /// Last-tick cache-hit cost in USD.
    pub tick_cost_hit_usd: f64,
    /// Last-tick cache-miss cost in USD.
    pub tick_cost_miss_usd: f64,
    /// Last-tick output cost in USD.
    pub tick_cost_output_usd: f64,

    // === Budget / guard rails ===
    /// Cleaning threshold (0.0–1.0): triggers auto-cleaning when exceeded.
    pub cleaning_threshold: f32,
    /// Context budget in tokens (`None` = model's full window).
    pub context_budget: Option<usize>,
    /// Current API retry count (reset on success).
    pub api_retry_count: u32,
    /// Guard-rail block reason (set when the spine blocks, cleared on stream start).
    pub guard_rail_blocked: Option<String>,
    /// Sleep timer: the tool pipeline waits until this ms timestamp.
    pub tool_sleep_until_ms: u64,

    // === Cache / freeze engine state (duplicated verbatim per thread) ===
    /// Previous panel hash list for cache cost tracking.
    pub previous_panel_hash_list: Vec<String>,
    /// Saved panel ID order from the last emitted tick (freeze stability).
    pub previous_panel_order: Vec<String>,
    /// Panel ID → context type from the last emitted tick (disappearance detection).
    pub previous_panel_id_types: Vec<(String, String)>,
    /// Panel IDs that carried a cache breakpoint on the last emitted tick.
    pub previous_breakpoint_panel_ids: Vec<String>,
    /// Full snapshot of panel `ContextItem`s from the last unfrozen tick.
    pub frozen_context_snapshot: Option<Vec<ContextItem>>,
    /// Cache optimization engine serialized state (breakpoint placement).
    pub cache_engine_json: Option<String>,
    /// Tempo flag: `true` means "nothing changed — freeze everything next tick".
    pub tempo: bool,
    /// Pre-tick telemetry for the cost-tracking TSV.
    pub tick_telemetry: Option<TickTelemetry>,
    /// Number of alive (non-pruned) breakpoints at the last tick (sidebar).
    pub tick_alive_breakpoints: usize,
    /// Per-mille positions (0–1000) of alive breakpoints within the prompt.
    pub tick_alive_bp_positions: Vec<u16>,

    // === Render cache (runtime-only) ===
    /// Last viewport width used for render-cache invalidation.
    pub last_viewport_width: u16,
    /// Cached rendered lines per message ID.
    pub message_cache: HashMap<String, MessageCache>,
    /// Cached rendered lines for the input area.
    pub input_cache: Option<InputCache>,
    /// Full content cache (entire conversation output).
    pub full_content_cache: Option<FullCache>,

    // === Per-thread module data ===
    /// The resident thread's per-thread module `TypeMap` (spine inbox, queue,
    /// console ownership, watcher registry, search/git views, …).
    pub thread_module_data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Default for ThreadRuntime {
    // Flat per-thread runtime initializer: one `field: value` line each, mirroring
    // the per-thread subset of the flat `State::default` literal. Grouping fields
    // into sub-structs to shave lines would diverge from the flat `State` layout
    // that `swap_with` targets field-by-field, so it stays flat and carries the
    // length expect (threshold 60, unchanged) — the twin of the State::default expect.
    #[expect(
        clippy::too_many_lines,
        reason = "flat per-thread runtime initializer mirroring State::default; sub-grouping would diverge from the flat State layout the swap targets field-by-field"
    )]
    fn default() -> Self {
        // Mirrors the per-thread field defaults in `State::default` so a freshly
        // created thread starts exactly like today's single resident thread.
        Self {
            context: vec![],
            messages: vec![],
            input: String::new(),
            input_cursor: 0,
            input_selection_anchor: None,
            paste_buffers: vec![],
            paste_buffer_labels: vec![],
            selected_context: 0,
            scroll_offset: 0.0,
            scroll_accel: 1.0,
            max_scroll: 0.0,
            stream: StreamState::default(),
            streaming_tool: None,
            last_stop_reason: None,
            streaming_estimated_tokens: 0,
            waiting_for_panels: false,
            next_user_id: 1,
            next_assistant_id: 1,
            next_tool_id: 1,
            next_result_id: 1,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            total_output_tokens: 0,
            uncached_input_tokens: 0,
            stream_cache_hit_tokens: 0,
            stream_cache_miss_tokens: 0,
            stream_output_tokens: 0,
            stream_uncached_input_tokens: 0,
            tick_cache_hit_tokens: 0,
            tick_cache_miss_tokens: 0,
            tick_output_tokens: 0,
            tick_uncached_input_tokens: 0,
            cost_hit_usd: 0.0,
            cost_miss_usd: 0.0,
            cost_output_usd: 0.0,
            stream_cost_hit_usd: 0.0,
            stream_cost_miss_usd: 0.0,
            stream_cost_output_usd: 0.0,
            tick_cost_hit_usd: 0.0,
            tick_cost_miss_usd: 0.0,
            tick_cost_output_usd: 0.0,
            cleaning_threshold: 0.70,
            context_budget: None,
            api_retry_count: 0,
            guard_rail_blocked: None,
            tool_sleep_until_ms: 0,
            previous_panel_hash_list: vec![],
            previous_panel_order: vec![],
            previous_panel_id_types: vec![],
            previous_breakpoint_panel_ids: vec![],
            frozen_context_snapshot: None,
            cache_engine_json: None,
            tempo: true,
            tick_telemetry: None,
            tick_alive_breakpoints: 0,
            tick_alive_bp_positions: vec![],
            last_viewport_width: 0,
            message_cache: HashMap::new(),
            input_cache: None,
            full_content_cache: None,
            thread_module_data: HashMap::new(),
        }
    }
}

impl std::fmt::Debug for ThreadRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadRuntime")
            .field("context_len", &self.context.len())
            .field("messages_len", &self.messages.len())
            .field("stream_phase", &self.stream.phase)
            .field("thread_module_data_keys", &self.thread_module_data.len())
            .finish_non_exhaustive()
    }
}

impl ThreadRuntime {
    /// A fresh, empty thread runtime (every field at its single-thread default).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Exchange every per-thread field with `state` in O(1).
    ///
    /// The operation is symmetric: calling it once *loads* `self` into `state`
    /// (and parks `state`'s previous resident context back into `self`); calling
    /// it again with the same pair restores the original arrangement. The loop
    /// uses this to make a non-resident thread temporarily resident for one
    /// advancement step, then swap the focused thread back.
    ///
    /// Only the per-thread subset moves — fleet-shared fields on `State` (tools,
    /// active modules, theme, provider/model, the shared module `TypeMap`,
    /// `global_next_uid`, the highlight fn, reveries) are untouched.
    #[expect(
        clippy::too_many_lines,
        reason = "flat per-field mem::swap mirroring every per-thread State field; one swap per field, nothing to factor without a macro that re-expands to the same body"
    )]
    pub fn swap_with(&mut self, state: &mut State) {
        use std::mem::swap;

        swap(&mut self.context, &mut state.context);
        swap(&mut self.messages, &mut state.messages);

        swap(&mut self.input, &mut state.input);
        swap(&mut self.input_cursor, &mut state.input_cursor);
        swap(&mut self.input_selection_anchor, &mut state.input_selection_anchor);
        swap(&mut self.paste_buffers, &mut state.paste_buffers);
        swap(&mut self.paste_buffer_labels, &mut state.paste_buffer_labels);
        swap(&mut self.selected_context, &mut state.selected_context);
        swap(&mut self.scroll_offset, &mut state.scroll_offset);
        swap(&mut self.scroll_accel, &mut state.scroll_accel);
        swap(&mut self.max_scroll, &mut state.max_scroll);

        swap(&mut self.stream, &mut state.flags.stream);
        swap(&mut self.streaming_tool, &mut state.streaming_tool);
        swap(&mut self.last_stop_reason, &mut state.last_stop_reason);
        swap(&mut self.streaming_estimated_tokens, &mut state.streaming_estimated_tokens);
        swap(&mut self.waiting_for_panels, &mut state.flags.lifecycle.waiting_for_panels);

        swap(&mut self.next_user_id, &mut state.next_user_id);
        swap(&mut self.next_assistant_id, &mut state.next_assistant_id);
        swap(&mut self.next_tool_id, &mut state.next_tool_id);
        swap(&mut self.next_result_id, &mut state.next_result_id);

        swap(&mut self.cache_hit_tokens, &mut state.cache_hit_tokens);
        swap(&mut self.cache_miss_tokens, &mut state.cache_miss_tokens);
        swap(&mut self.total_output_tokens, &mut state.total_output_tokens);
        swap(&mut self.uncached_input_tokens, &mut state.uncached_input_tokens);
        swap(&mut self.stream_cache_hit_tokens, &mut state.stream_cache_hit_tokens);
        swap(&mut self.stream_cache_miss_tokens, &mut state.stream_cache_miss_tokens);
        swap(&mut self.stream_output_tokens, &mut state.stream_output_tokens);
        swap(&mut self.stream_uncached_input_tokens, &mut state.stream_uncached_input_tokens);
        swap(&mut self.tick_cache_hit_tokens, &mut state.tick_cache_hit_tokens);
        swap(&mut self.tick_cache_miss_tokens, &mut state.tick_cache_miss_tokens);
        swap(&mut self.tick_output_tokens, &mut state.tick_output_tokens);
        swap(&mut self.tick_uncached_input_tokens, &mut state.tick_uncached_input_tokens);

        swap(&mut self.cost_hit_usd, &mut state.cost_hit_usd);
        swap(&mut self.cost_miss_usd, &mut state.cost_miss_usd);
        swap(&mut self.cost_output_usd, &mut state.cost_output_usd);
        swap(&mut self.stream_cost_hit_usd, &mut state.stream_cost_hit_usd);
        swap(&mut self.stream_cost_miss_usd, &mut state.stream_cost_miss_usd);
        swap(&mut self.stream_cost_output_usd, &mut state.stream_cost_output_usd);
        swap(&mut self.tick_cost_hit_usd, &mut state.tick_cost_hit_usd);
        swap(&mut self.tick_cost_miss_usd, &mut state.tick_cost_miss_usd);
        swap(&mut self.tick_cost_output_usd, &mut state.tick_cost_output_usd);

        swap(&mut self.cleaning_threshold, &mut state.cleaning_threshold);
        swap(&mut self.context_budget, &mut state.context_budget);
        swap(&mut self.api_retry_count, &mut state.api_retry_count);
        swap(&mut self.guard_rail_blocked, &mut state.guard_rail_blocked);
        swap(&mut self.tool_sleep_until_ms, &mut state.tool_sleep_until_ms);

        swap(&mut self.previous_panel_hash_list, &mut state.previous_panel_hash_list);
        swap(&mut self.previous_panel_order, &mut state.previous_panel_order);
        swap(&mut self.previous_panel_id_types, &mut state.previous_panel_id_types);
        swap(&mut self.previous_breakpoint_panel_ids, &mut state.previous_breakpoint_panel_ids);
        swap(&mut self.frozen_context_snapshot, &mut state.frozen_context_snapshot);
        swap(&mut self.cache_engine_json, &mut state.cache_engine_json);
        swap(&mut self.tempo, &mut state.tempo);
        swap(&mut self.tick_telemetry, &mut state.tick_telemetry);
        swap(&mut self.tick_alive_breakpoints, &mut state.tick_alive_breakpoints);
        swap(&mut self.tick_alive_bp_positions, &mut state.tick_alive_bp_positions);

        swap(&mut self.last_viewport_width, &mut state.last_viewport_width);
        swap(&mut self.message_cache, &mut state.message_cache);
        swap(&mut self.input_cache, &mut state.input_cache);
        swap(&mut self.full_content_cache, &mut state.full_content_cache);

        swap(&mut self.thread_module_data, &mut state.thread_module_data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_loads_into_state() {
        let mut rt = ThreadRuntime::new();
        rt.input = "hello".to_owned();
        rt.next_user_id = 42;
        rt.total_output_tokens = 1000;
        rt.tempo = false;

        let mut state = State::default();
        rt.swap_with(&mut state);

        // State now holds the runtime's values, and the runtime holds what State
        // used to hold (the defaults). Tuple compares keep this one branch.
        assert_eq!(
            (state.input.as_str(), state.next_user_id, state.total_output_tokens, state.tempo),
            ("hello", 42, 1000, false)
        );
        assert_eq!((rt.input.as_str(), rt.next_user_id, rt.tempo), ("", 1, true));
    }

    #[test]
    fn swap_is_symmetric() {
        let mut rt = ThreadRuntime::new();
        rt.input = "hello".to_owned();
        rt.next_user_id = 42;

        let mut state = State::default();
        rt.swap_with(&mut state);
        rt.swap_with(&mut state);

        // Two swaps restore the original arrangement.
        assert_eq!((state.input.as_str(), state.next_user_id), ("", 1));
        assert_eq!((rt.input.as_str(), rt.next_user_id), ("hello", 42));
    }

    #[test]
    fn thread_module_data_travels() {
        let mut rt = ThreadRuntime::new();
        let mut state = State::default();
        state.set_ext_thread(99u32);
        assert_eq!(state.get_ext::<u32>(), Some(&99));

        // Swapping moves the per-thread module data out of state…
        rt.swap_with(&mut state);
        let parked = rt.thread_module_data.get(&TypeId::of::<u32>()).and_then(|b| b.downcast_ref::<u32>());
        assert_eq!((state.get_ext::<u32>(), parked), (None, Some(&99)));

        // …and swapping back restores it.
        rt.swap_with(&mut state);
        assert_eq!(state.get_ext::<u32>(), Some(&99));
    }
}
