use cp_base::state::data::model_helpers::ModelPricing as _;
use std::sync::mpsc::{Receiver, Sender};

use crate::app::actions::{Action, ActionResult, apply_action};
use crate::infra::api::{StreamEvent, start_streaming};
use crate::infra::constants::MAX_API_RETRIES;

use crate::app::App;
use crate::app::context::{build_stream_params, get_active_agent_content, prepare_stream_context};
use crate::state::cache::{CacheUpdate, process_cache_request};
use crate::state::{State, StreamPhase, get_context_type_meta};

/// Drain the stream-event channel and apply each event (chunks, tools, done, errors).
pub(super) fn process_stream_events(app: &mut App, rx: &Receiver<StreamEvent>) {
    let _guard = crate::profile!("app::stream_events");
    let _fg = cp_base::flame!("stream");
    while let Ok(evt) = rx.try_recv() {
        if !app.state.flags.stream.phase.is_streaming() {
            continue;
        }
        app.state.flags.ui.dirty = true;
        apply_stream_event(app, evt);
    }
}

/// Apply one drained [`StreamEvent`] to app state.
///
/// An `if let` chain (not an exhaustive match) so `StreamEvent` stays
/// `#[non_exhaustive]`; the loop-bearing arms delegate to `broadcast_*` helpers
/// to keep this dispatcher's cognitive complexity under the cap.
fn apply_stream_event(app: &mut App, evt: StreamEvent) {
    if let StreamEvent::Chunk(text) = evt {
        broadcast_chunk(app, &text);
    } else if let StreamEvent::ToolProgress { name, input_so_far } = evt {
        broadcast_tool_progress(app, name, input_so_far);
    } else if let StreamEvent::ToolUse(tool) = evt {
        broadcast_tool_use(app, tool);
    } else if let StreamEvent::Done {
        input_tokens,
        output_tokens,
        cache_hit_tokens,
        cache_miss_tokens,
        stop_reason,
        bp_hashes,
        bp_panel_ids,
        alive_count,
        alive_positions_permille,
    } = evt
    {
        app.typewriter.mark_done();
        app.state.streaming_tool = None;
        app.pending_done = Some((
            input_tokens,
            output_tokens,
            cache_hit_tokens,
            cache_miss_tokens,
            stop_reason,
            bp_hashes,
            bp_panel_ids,
            alive_count,
            alive_positions_permille,
        ));
        // API call succeeded — reset retry counter immediately at tick level
        app.state.api_retry_count = 0;
    } else if let StreamEvent::Error(e) = evt {
        handle_stream_error_event(app, e);
    } else {
        // Future non_exhaustive variants: ignored by the streaming pipeline.
    }
}

/// Broadcast a text chunk to every module, then feed the typewriter.
fn broadcast_chunk(app: &mut App, text: &str) {
    for module in crate::modules::all_modules() {
        module.on_stream_chunk(text, &mut app.state);
    }
    app.typewriter.add_chunk(text);
}

/// Notify modules of streaming tool progress (e.g. typing indicators) and stash
/// the advisory streaming-tool preview.
fn broadcast_tool_progress(app: &mut App, name: String, input_so_far: String) {
    for module in crate::modules::all_modules() {
        module.on_tool_progress(&name, &input_so_far, &mut app.state);
    }
    app.state.streaming_tool = Some(crate::state::StreamingTool::new(name, input_so_far));
}

/// Notify modules a tool call completed (e.g. clear typing), clear the preview,
/// and queue the tool for execution.
fn broadcast_tool_use(app: &mut App, tool: cp_base::tools::ToolUse) {
    for module in crate::modules::all_modules() {
        module.on_tool_complete(&tool.name, &mut app.state);
    }
    app.state.streaming_tool = None;
    app.pending_tools.push(tool);
}

/// Handle a `StreamEvent::Error`: log to disk, then either flag a retry (under
/// the retry cap) or surface the error + record a continuation-error backoff.
fn handle_stream_error_event(app: &mut App, e: String) {
    app.typewriter.reset();
    // Log every error to disk for debugging
    let attempt = app.state.api_retry_count.saturating_add(1);
    let will_retry = attempt <= MAX_API_RETRIES;
    let provider = format!("{:?}", app.state.llm_provider);
    let model = app.state.current_model();
    let log_msg = format!(
        "Attempt {}/{} ({})\n\
         Provider: {} | Model: {}\n\
         Last request dump: .context-pilot/last_requests/\n\n\
         {}\n",
        attempt,
        MAX_API_RETRIES + 1,
        if will_retry { "will retry" } else { "giving up" },
        provider,
        model,
        e
    );
    let _log = crate::state::persistence::log_error(&log_msg);

    // Check if we should retry
    if will_retry {
        app.state.api_retry_count = app.state.api_retry_count.saturating_add(1);
        app.pending_retry_error = Some(e);
    } else {
        // Max retries reached, show error
        app.state.api_retry_count = 0;
        // Track consecutive failed continuations for backoff
        let spine = cp_mod_spine::types::SpineState::get_mut(&mut app.state);
        spine.config.consecutive_continuation_errors = spine.config.consecutive_continuation_errors.saturating_add(1);
        spine.config.last_continuation_error_ms = Some(crate::app::panels::now_ms());
        let _action = apply_action(&mut app.state, Action::StreamError(e));
    }
}

/// If a retryable error is pending, clear partial state and re-launch the stream.
pub(super) fn handle_retry(app: &mut App, tx: &Sender<StreamEvent>) {
    if let Some(_error) = app.pending_retry_error.take() {
        // Still streaming, retry the request
        if app.state.flags.stream.phase.is_streaming() {
            // Clear any partial assistant message content before retrying
            if let Some(msg) = app.state.messages.last_mut()
                && msg.role == "assistant"
            {
                msg.content.clear();
            }
            let ctx = prepare_stream_context(&mut app.state, true, None);
            let system_prompt = get_active_agent_content(&app.state);
            app.typewriter.reset();
            app.pending_done = None;
            let params = build_stream_params(&app.state, ctx, Some(system_prompt));
            start_streaming(params, tx.clone());
            app.state.flags.ui.dirty = true;
        }
    }
}

/// Flush buffered typewriter characters into the assistant message.
pub(super) fn process_typewriter(app: &mut App) {
    let _guard = crate::profile!("app::typewriter");
    if app.state.flags.stream.phase.is_streaming()
        && let Some(chars) = app.typewriter.take_chars()
    {
        let _r = apply_action(&mut app.state, Action::AppendChars(chars));
        app.state.flags.ui.dirty = true;
    }
}

/// Poll for completed API-key validation results and store them in state.
pub(super) fn process_api_check_results(app: &mut App) {
    if let Some(rx) = app.api_check_rx.as_ref()
        && let Ok(result) = rx.try_recv()
    {
        app.state.flags.lifecycle.api_check_in_progress = false;
        app.state.api_check_result = Some(result);
        app.state.flags.ui.dirty = true;
        app.api_check_rx = None;
        app.save_state_async();
    }
}

/// Continue streaming after tool execution (called when panels are ready).
pub(super) fn continue_streaming(app: &mut App, tx: &Sender<StreamEvent>) {
    app.state.flags.stream.phase.transition(StreamPhase::Receiving);
    let ctx = prepare_stream_context(&mut app.state, true, None);
    let system_prompt = get_active_agent_content(&app.state);
    app.typewriter.reset();
    app.pending_done = None;
    let params = build_stream_params(&app.state, ctx, Some(system_prompt));
    start_streaming(params, tx.clone());
}

/// Finalize a completed stream: apply `StreamDone`, reset counters, and unblock spine.
pub(super) fn finalize_stream(app: &mut App) {
    let _fg = cp_base::flame!("finalize_stream");
    if !app.state.flags.stream.phase.is_streaming() {
        return;
    }
    // Don't finalize while waiting for panels or deferred sleep —
    // pending_done is still Some from the intermediate stream, and
    // continue_streaming will clear it when the deferred state resolves.
    if app.state.flags.lifecycle.waiting_for_panels || app.deferred_tool_sleeping {
        return;
    }
    // Don't finalize while a console blocking wait is pending
    if app.pending_console_wait_tool_results.is_some() {
        return;
    }

    let ready = app.typewriter.pending_chars.is_empty() && app.pending_tools.is_empty();
    if ready && let Some(done) = app.pending_done.take() {
        apply_stream_done(app, done);
        app.typewriter.reset();
        app.pending_done = None;
    }
}

/// Full nine-field payload of a completed stream (`StreamEvent::Done`), taken
/// from `App::pending_done` when the tick is ready to finalize.
type StreamDonePayload = (usize, usize, usize, usize, Option<String>, Vec<String>, Vec<String>, usize, Vec<u16>);

/// Apply a completed stream's `Done` payload: fold token/cost totals via
/// `Action::StreamDone` (persisting the message + state per the result), reset
/// the spine auto-continuation + error-backoff counters, unblock guard-railed
/// notifications, and record the cache breakpoints for next turn.
fn apply_stream_done(app: &mut App, done: StreamDonePayload) {
    let (
        input_tokens,
        output_tokens,
        cache_hit_tokens,
        cache_miss_tokens,
        stop_reason,
        bp_hashes,
        bp_panel_ids,
        alive_count,
        alive_positions_permille,
    ) = done;

    app.state.flags.ui.dirty = true;
    // `if let` chain (not an exhaustive match) so ActionResult stays #[non_exhaustive].
    let result = apply_action(
        &mut app.state,
        Action::StreamDone { input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason },
    );
    if let ActionResult::SaveMessage(id) = result {
        if let Some(msg) = app.state.messages.iter().find(|m| m.id == id) {
            app.save_message_async(msg);
        }
        app.save_state_async();
    } else if matches!(result, ActionResult::Save) {
        app.save_state_async();
    } else {
        // Nothing / StopStream / StartApiCheck + future non_exhaustive variants: no-op here.
    }

    // Reset auto-continuation count on each successful tick (stream completion).
    // This means MaxAutoRetries only fires on consecutive *failed* continuations,
    // not on total auto-continuations in an autonomous session.
    {
        let spine_cfg = &mut cp_mod_spine::types::SpineState::get_mut(&mut app.state).config;
        spine_cfg.auto_continuation_count = 0;
        // Reset consecutive error backoff — successful completion proves API is healthy
        spine_cfg.consecutive_continuation_errors = 0;
        spine_cfg.last_continuation_error_ms = None;
    }

    // Unblock any guard-rail-blocked notifications — they get another chance now
    // that a stream has completed successfully.
    cp_mod_spine::types::SpineState::unblock_all(&mut app.state);

    record_stream_breakpoints(app, BreakpointRecord { bp_hashes, bp_panel_ids, alive_count, alive_positions_permille });
}

/// This turn's cache-breakpoint telemetry, bundled to keep
/// [`record_stream_breakpoints`] under the argument cap.
struct BreakpointRecord {
    /// Accumulated hashes sent as breakpoints this turn.
    bp_hashes: Vec<String>,
    /// Panel id each breakpoint landed on, in prompt order.
    bp_panel_ids: Vec<String>,
    /// How many stored breakpoints matched the current hash chain.
    alive_count: usize,
    /// Per-mille positions (0–1000) of alive breakpoints within the prompt.
    alive_positions_permille: Vec<u16>,
}

/// Fold this turn's cache breakpoints into the persisted cache engine (prune +
/// record) and stash the alive-BP telemetry + culprit panel ids for the next
/// turn's freeze pass. No-op when no breakpoints were sent.
fn record_stream_breakpoints(app: &mut App, record: BreakpointRecord) {
    let BreakpointRecord { bp_hashes, bp_panel_ids, alive_count, alive_positions_permille } = record;
    // Update cache optimization engine with breakpoint hashes from this request.
    // This records which accumulated hashes were sent as breakpoints, so the next
    // request can detect the cache frontier and place breakpoints optimally.
    if !bp_hashes.is_empty() {
        let now_ms = cp_base::panels::now_ms();
        let mut engine = app.state.cache_engine_json.as_deref().map_or_else(
            crate::llms::cache::cache_engine::CacheEngine::default,
            crate::llms::cache::cache_engine::CacheEngine::from_json,
        );
        engine.prune(now_ms);
        engine.record_breakpoints(&bp_hashes, now_ms);
        app.state.tick_alive_breakpoints = alive_count;
        app.state.tick_alive_bp_positions = alive_positions_permille;
        app.state.cache_engine_json = Some(engine.to_json());
    }

    // Record which panels carried a breakpoint this turn — the freeze pass
    // reads it next turn to widen the free-to-update region back to the last
    // alive breakpoint before the culprit (BP-anchored free region).
    app.state.previous_breakpoint_panel_ids = bp_panel_ids;
}

// ─── Panel Wait Helpers ─────────────────────────────────────────────────────

/// Check if any async-wait panels have `cache_deprecated` = true.
pub(super) fn has_dirty_panels(state: &State) -> bool {
    state.context.iter().any(|c| {
        get_context_type_meta(c.context_type.as_str()).is_some_and(|m| m.needs_async_wait) && c.cache_deprecated
    })
}

/// Check if any async-wait panels need refresh before continuing the stream.
pub(super) fn has_dirty_file_panels(state: &State) -> bool {
    state.context.iter().any(|c| {
        get_context_type_meta(c.context_type.as_str()).is_some_and(|m| m.needs_async_wait) && c.cache_deprecated
    })
}

/// Trigger immediate cache refresh for all dirty async-wait panels.
/// Returns true if any panels needed refresh.
pub(super) fn trigger_dirty_panel_refresh(state: &State, cache_tx: &Sender<CacheUpdate>) -> bool {
    let mut any_triggered = false;
    for ctx in &state.context {
        let needs_wait = get_context_type_meta(ctx.context_type.as_str()).is_some_and(|m| m.needs_async_wait);
        if needs_wait && ctx.cache_deprecated && !ctx.cache_in_flight {
            let panel = crate::app::panels::get_panel(&ctx.context_type);
            if let Some(request) = panel.build_cache_request(ctx, state) {
                process_cache_request(request, cache_tx.clone());
                any_triggered = true;
            }
        }
    }
    any_triggered
}
