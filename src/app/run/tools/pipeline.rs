use std::sync::mpsc::Sender;

use crate::app::actions::clean_llm_id_prefix;
use crate::app::panels::now_ms;
use crate::infra::api::StreamEvent;
use crate::infra::tools::execute_tool;
use crate::modules::pre_flight::pre_flight_tool;
use crate::state::persistence::build_message_op;
use crate::state::{Message, StreamPhase, ToolResultRecord, ToolUseRecord};

use crate::app::run::streaming::{has_dirty_file_panels, trigger_dirty_panel_refresh};
use cp_base::state::data::model_helpers::{ModelPricing as _, token_cost};
use cp_mod_console::tools::CONSOLE_WAIT_BLOCKING_SENTINEL;
use cp_mod_queue::types::QueueState;

use crate::app::App;
use std::fmt::Write as _;

// ─── Tool pipeline ──────────────────────────────────────────────────────────

/// Accumulate token stats AND costs from the intermediate stream into tick/stream/total counters.
///
/// Called before `continue_streaming()` for tool-use ticks — the intermediate
/// `pending_done` would otherwise be lost (only the final tick goes through
/// `finalize_stream → handle_stream_done → apply_token_usage`).
pub(crate) fn accumulate_pending_token_stats(app: &mut App) {
    if let Some((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, _, _, _, _, _)) = app.pending_done {
        // Fold uncached input into cache_miss for correct cost accounting
        let effective_miss = cache_miss_tokens.saturating_add(input_tokens);

        // --- Token accumulation ---
        app.state.tick_cache_hit_tokens = cache_hit_tokens;
        app.state.tick_cache_miss_tokens = effective_miss;
        app.state.tick_output_tokens = output_tokens;
        app.state.tick_uncached_input_tokens = input_tokens;
        app.state.stream_cache_hit_tokens = app.state.stream_cache_hit_tokens.saturating_add(cache_hit_tokens);
        app.state.stream_cache_miss_tokens = app.state.stream_cache_miss_tokens.saturating_add(effective_miss);
        app.state.stream_output_tokens = app.state.stream_output_tokens.saturating_add(output_tokens);
        app.state.stream_uncached_input_tokens = app.state.stream_uncached_input_tokens.saturating_add(input_tokens);
        app.state.cache_hit_tokens = app.state.cache_hit_tokens.saturating_add(cache_hit_tokens);
        app.state.cache_miss_tokens = app.state.cache_miss_tokens.saturating_add(effective_miss);
        app.state.total_output_tokens = app.state.total_output_tokens.saturating_add(output_tokens);
        app.state.uncached_input_tokens = app.state.uncached_input_tokens.saturating_add(input_tokens);

        // --- Cost accumulation (frozen at consumption-time pricing) ---
        let cost_hit = token_cost(cache_hit_tokens, app.state.cache_hit_price_per_mtok());
        let cost_miss = cp_base::cast::float_math::add(
            token_cost(cache_miss_tokens, app.state.cache_miss_price_per_mtok()),
            token_cost(input_tokens, app.state.input_price_per_mtok()),
        );
        let cost_output = token_cost(output_tokens, app.state.output_price_per_mtok());

        app.state.tick_cost_hit_usd = cost_hit;
        app.state.tick_cost_miss_usd = cost_miss;
        app.state.tick_cost_output_usd = cost_output;
        app.state.stream_cost_hit_usd = cp_base::cast::float_math::add(app.state.stream_cost_hit_usd, cost_hit);
        app.state.stream_cost_miss_usd = cp_base::cast::float_math::add(app.state.stream_cost_miss_usd, cost_miss);
        app.state.stream_cost_output_usd =
            cp_base::cast::float_math::add(app.state.stream_cost_output_usd, cost_output);
        app.state.cost_hit_usd = cp_base::cast::float_math::add(app.state.cost_hit_usd, cost_hit);
        app.state.cost_miss_usd = cp_base::cast::float_math::add(app.state.cost_miss_usd, cost_miss);
        app.state.cost_output_usd = cp_base::cast::float_math::add(app.state.cost_output_usd, cost_output);
    }
}

/// Create and persist a `tool_call` message for a single `ToolUse`.
/// Used for both direct tool calls and queue-flushed replays.
fn save_tool_call_message(app: &mut App, tool: &cp_base::tools::ToolUse) {
    let tool_id = format!("T{}", app.state.next_tool_id);
    let tool_global_uid = format!("UID_{}_T", app.state.global_next_uid);
    app.state.next_tool_id = app.state.next_tool_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

    let tool_msg = Message::new_tool_call(
        tool_id,
        Some(tool_global_uid),
        vec![ToolUseRecord::new(tool.id.clone(), tool.name.clone(), tool.input.clone())],
    );
    app.save_message_async(&tool_msg);
    app.state.messages.push(tool_msg);
}

/// Execute one tool through the pre-flight → queue-intercept → execute pipeline.
///
/// Handles the four dispatch classes (queue control, trap, queue-intercept,
/// normal execute) and appends any queue-flushed tools onto `flushed_tools`.
fn execute_one_tool(
    app: &mut App,
    tool: &cp_base::tools::ToolUse,
    flushed_tools: &mut Vec<super::queue_flush::FlushedTool>,
) -> crate::infra::tools::ToolResult {
    if tool.name == "Queue_execute" || tool.name == "Queue_pause" {
        return execute_queue_control(app, tool, flushed_tools);
    }

    // Pre-flight: schema check + module semantic check (ALWAYS runs, queue or not)
    let pf = pre_flight_tool(tool, &app.state, &app.state.active_modules.clone());
    if pf.has_errors() {
        // Hard stop — don't queue, don't execute
        return crate::infra::tools::ToolResult::new(tool.id.clone(), pf.format_errors(), true);
    }

    // Pre-flight may request queue activation (e.g. destructive operations)
    if pf.activate_queue {
        let qs = QueueState::get_mut(&mut app.state);
        qs.active = true;
    }

    let should_queue =
        QueueState::get(&app.state).active && !QueueState::is_queue_tool(&tool.name) && tool.name != "Think";
    if should_queue {
        return enqueue_tool(app, tool, &pf, flushed_tools);
    }

    // Execute normally
    let mut result = execute_tool(tool, &mut app.state);
    if pf.has_warnings() {
        let _r = write!(result.content, "\n{}", pf.format_errors());
    }
    result
}

/// Handle `Queue_execute` / `Queue_pause`: trap gate, flush, or pause-execute.
fn execute_queue_control(
    app: &mut App,
    tool: &cp_base::tools::ToolUse,
    flushed_tools: &mut Vec<super::queue_flush::FlushedTool>,
) -> crate::infra::tools::ToolResult {
    // History cleanup trap: block flush/pause when too many history panels are
    // open. Queue_pause is checked here too — otherwise the LLM can pause the
    // queue then execute tools normally, bypassing the trap (which only fires on
    // Queue_execute).
    if let Some(trap_msg) = crate::modules::conversation_history::trap::check_and_trigger_trap(&mut app.state) {
        return crate::infra::tools::ToolResult::new(tool.id.clone(), trap_msg, false);
    }
    if tool.name == "Queue_execute" {
        // Queue flush: execute all queued calls, collect them for pipeline replay
        let (summary_result, flushed) = super::queue_flush::execute_queue_flush(tool, &mut app.state);
        *flushed_tools = flushed;
        return summary_result;
    }
    // Queue_pause — no trap conditions met, execute normally
    let pf = pre_flight_tool(tool, &app.state, &app.state.active_modules.clone());
    if pf.has_errors() {
        return crate::infra::tools::ToolResult::new(tool.id.clone(), pf.format_errors(), true);
    }
    let mut result = execute_tool(tool, &mut app.state);
    if pf.has_warnings() {
        let _r = write!(result.content, "\n{}", pf.format_errors());
    }
    result
}

/// Enqueue an intercepted tool, auto-flushing when a queued
/// `Close_conversation_history` would defuse the history-cleanup trap.
fn enqueue_tool(
    app: &mut App,
    tool: &cp_base::tools::ToolUse,
    pf: &cp_base::tools::pre_flight::Verdict,
    flushed_tools: &mut Vec<super::queue_flush::FlushedTool>,
) -> crate::infra::tools::ToolResult {
    let qs = QueueState::get_mut(&mut app.state);
    let idx =
        qs.enqueue(cp_mod_queue::types::QueuedToolCall::new(tool.name.clone(), tool.id.clone(), tool.input.clone()));
    let mut msg = format!("Queued as #{idx}");
    if pf.has_warnings() {
        let _r = write!(msg, "\n{}", pf.format_errors());
    }

    // Auto-flush: a queued Close_conversation_history that would defuse the trap
    // deactivates it and flushes the entire queue (the original Queue_execute intent).
    if tool.name == "Close_conversation_history"
        && crate::modules::conversation_history::trap::would_defuse_trap(&app.state)
    {
        crate::modules::conversation_history::trap::force_deactivate_trap(&mut app.state);
        let (flush_result, flushed) = super::queue_flush::execute_queue_flush(tool, &mut app.state);
        let n = flushed.len();
        *flushed_tools = flushed;
        return crate::infra::tools::ToolResult::new(
            tool.id.clone(),
            format!("{msg}\nTrap defused — auto-executing {n} queued action(s).\n{}", flush_result.content),
            false,
        );
    }
    crate::infra::tools::ToolResult::new(tool.id.clone(), msg, false)
}

/// Run the three follow-ups triggered by a `Close_conversation_history` tool:
/// trap deactivation, remaining-panel augmentation, and tree folder recompute.
fn run_history_close_followups(
    app: &mut App,
    tools: &[cp_base::tools::ToolUse],
    tool_results: &mut [crate::infra::tools::ToolResult],
) {
    if !tools.iter().any(|t| t.name == "Close_conversation_history") {
        return;
    }
    crate::modules::conversation_history::trap::maybe_deactivate_trap(&mut app.state);
    super::queue_flush::augment_remaining_history_panels(&app.state, tools, tool_results);
    crate::modules::conversation_history::recompute_toggled_tree_folders::recompute_tree_folders(&mut app.state);
}

/// Sync new logs to Meilisearch and push Think `task_context` signals into the
/// Context Radar ring buffer, refreshing the radar when anything changed.
fn sync_logs_and_radar(
    app: &mut App,
    tools: &[cp_base::tools::ToolUse],
    tool_results: &[crate::infra::tools::ToolResult],
) {
    let logs_changed = tools.iter().zip(tool_results.iter()).any(|(t, r)| {
        (t.name == "log_create" || t.name == "Close_conversation_history") && !r.content.starts_with("Queued as #")
    });
    if logs_changed {
        cp_mod_search::index::logsync::sync_logs_to_meilisearch(&app.state);
    }

    let mut radar_needs_refresh = logs_changed;
    for tool in tools {
        if tool.name != "Think" {
            continue;
        }
        let Some(ctx) = tool.input.get("task_context").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let trimmed = ctx.trim();
        if !trimmed.is_empty() {
            cp_mod_search::push_task_signal(&mut app.state, trimmed);
            radar_needs_refresh = true;
        }
    }

    if radar_needs_refresh {
        cp_mod_search::refresh_radar(&app.state);
    }
}

/// Start a reverie if any tool result carries a `REVERIE_START:` sentinel
/// (emitted by `optimize_context`).
fn maybe_trigger_reverie(app: &mut App, tool_results: &[crate::infra::tools::ToolResult]) {
    // Sentinel format: REVERIE_START:<agent_id>\n<context_or_empty>\n<human_readable_msg>
    for tr in tool_results {
        if let Some(rest) = tr.content.strip_prefix("REVERIE_START:") {
            let mut lines = rest.lines();
            let agent_id = lines.next().unwrap_or("cleaner").to_owned();
            let context_line = lines.next().unwrap_or("");
            let context = if context_line.is_empty() { None } else { Some(context_line.to_owned()) };
            let _r = crate::app::reverie::trigger::start_manual_reverie(&mut app.state, agent_id, context);
            break;
        }
    }
}

/// Break tempo unless every result opted out via `preserves_tempo` (blocking
/// sentinels defer the decision to the watcher).
fn apply_tempo_break(app: &mut App, tool_results: &[crate::infra::tools::ToolResult]) {
    for tr in tool_results {
        if tr.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL) {
            continue; // Deferred — watcher decides later
        }
        if !tr.preserves_tempo {
            app.state.tempo = false;
            break; // One break is enough
        }
    }
}

/// Guard, setup, tool-execution loop, and queue-flush replay — the front half
/// Owned outputs of [`collect_tool_results`]: the executed tools + their
/// results plus profiling bookkeeping, handed to the callback/finalize phases.
struct ToolBatch {
    /// Executed tool calls (including queue-flushed replays).
    tools: Vec<cp_base::tools::ToolUse>,
    /// Result per tool (order-aligned with `tools`).
    tool_results: Vec<crate::infra::tools::ToolResult>,
    /// Comma-joined tool names for slow-tool profiling.
    tool_names: String,
    /// Pipeline start instant for elapsed-time logging.
    pipeline_start: std::time::Instant,
}

/// of [`handle_tool_execution`]. Returns `None` when the pipeline should not run
/// this tick (not streaming, nothing pending, waiting on panels/sleep).
fn collect_tool_results(app: &mut App) -> Option<ToolBatch> {
    if !app.state.flags.stream.phase.is_streaming()
        || app.pending_done.is_none()
        || !app.typewriter.pending_chars.is_empty()
        || app.pending_tools.is_empty()
    {
        return None;
    }
    // Don't process new tools while waiting for panels or deferred sleep
    if app.state.flags.lifecycle.waiting_for_panels || app.deferred_tool_sleeping {
        return None;
    }

    app.state.flags.ui.dirty = true;
    app.state.flags.stream.phase.transition(StreamPhase::ExecutingTools);
    let mut tools = std::mem::take(&mut app.pending_tools);
    let mut tool_results: Vec<crate::infra::tools::ToolResult> = Vec::new();
    let mut flushed_tools: Vec<super::queue_flush::FlushedTool> = Vec::new();

    // Finalize current assistant message
    if let Some(msg) = app.state.messages.last_mut()
        && msg.role == "assistant"
    {
        // Clean any LLM ID prefixes before saving
        msg.content = clean_llm_id_prefix(&msg.content);
        let op = build_message_op(msg);
        app.writer.send_message(op);
    }

    // Create tool call messages and execute tools
    let pipeline_start = std::time::Instant::now();
    for tool in &tools {
        save_tool_call_message(app, tool);
        let result = execute_one_tool(app, tool, &mut flushed_tools);
        // Leave an auto tool-activity trace in the focused thread (no-op
        // unfocused) — after execution so the persisted record carries the
        // result, not just the params.
        crate::app::run::threads::maybe_append_tool_activity(&mut app.state, tool, &result);
        tool_results.push(result);
    }

    let tool_names: String = tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(",");

    // === QUEUE FLUSH REPLAY ===
    // If Queue_execute fired, extend the tools/results vecs with the flushed items,
    // so callbacks, sentinels, reload checks, and sleep detection see ALL tools.
    if !flushed_tools.is_empty() {
        for ft in &flushed_tools {
            super::queue_flush::save_flushed_tool_call_message(app, &ft.tool, ft.queue_index);
            // Trace each flushed tool AFTER its result is known (persisted
            // record carries params + result), mirroring the direct path.
            crate::app::run::threads::maybe_append_tool_activity(&mut app.state, &ft.tool, &ft.result);
        }
        for ft in flushed_tools {
            tools.push(ft.tool);
            tool_results.push(ft.result);
        }
    }

    Some(ToolBatch { tools, tool_results, tool_names, pipeline_start })
}

/// Execute pending tool calls: pre-flight, queue intercept, callbacks, and pipeline resumption.
pub(crate) fn handle_tool_execution(app: &mut App, tx: &Sender<StreamEvent>) {
    let _guard = crate::profile!("app::tool_exec");
    let _fg = cp_base::flame!("tool_pipeline");

    let Some(ToolBatch { tools, mut tool_results, tool_names, pipeline_start }) = collect_tool_results(app) else {
        return;
    };

    run_history_close_followups(app, &tools, &mut tool_results);
    sync_logs_and_radar(app, &tools, &tool_results);
    maybe_trigger_reverie(app, &tool_results);
    super::callbacks::fire_edit_callbacks(app, &tools, &mut tool_results);
    apply_tempo_break(app, &tool_results);

    // Check if any tool triggered a console blocking wait
    let has_console_wait = tool_results.iter().any(|r| r.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL));
    if has_console_wait {
        app.pending_console_wait_tool_results = Some(tool_results);
        app.save_state_async();
        crate::infra::profiler::log_tool_time(&tool_names, pipeline_start.elapsed());
        return;
    }

    finalize_tool_cycle(
        app,
        &ToolCycle { tx, tools: &tools, tool_results: &tool_results, tool_names: &tool_names, pipeline_start },
    );
}

/// Bundled inputs for [`finalize_tool_cycle`] — keeps the helper within the
/// 4-argument limit (`app` stays a separate `&mut` borrow).
struct ToolCycle<'cycle> {
    /// Stream event channel for resuming the LLM turn.
    tx: &'cycle Sender<StreamEvent>,
    /// Executed tool calls (order-aligned with `tool_results`).
    tools: &'cycle [cp_base::tools::ToolUse],
    /// Results for each tool (order-aligned with `tools`).
    tool_results: &'cycle [crate::infra::tools::ToolResult],
    /// Comma-joined tool names for slow-tool profiling.
    tool_names: &'cycle str,
    /// Pipeline start instant for elapsed-time logging.
    pipeline_start: std::time::Instant,
}

/// Build the `tool_result` user message, then create the next assistant
/// message, accumulate token stats, and resume streaming (unless a reload or
/// sleep defers it). The back half of [`handle_tool_execution`], split out to
/// keep the cognitive complexity within budget.
fn finalize_tool_cycle(app: &mut App, cycle: &ToolCycle<'_>) {
    let ToolCycle { tx, tools, tool_results, tool_names, pipeline_start } = *cycle;
    // Create tool result message
    let result_id = format!("R{}", app.state.next_result_id);
    let result_global_uid = format!("UID_{}_R", app.state.global_next_uid);
    app.state.next_result_id = app.state.next_result_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);
    let tool_result_records: Vec<ToolResultRecord> = tool_results
        .iter()
        .zip(tools.iter())
        .map(|(r, t)| {
            ToolResultRecord::new(r.tool_use_id.clone(), r.content.clone(), r.is_error)
                .display(r.display.clone())
                .tldr(r.tldr.clone())
                .tool_name(t.name.clone())
        })
        .collect();
    let result_msg = Message::new_tool_result(result_id, Some(result_global_uid), tool_result_records);
    app.save_message_async(&result_msg);
    app.state.messages.push(result_msg);

    // Check if reload was requested — main loop will handle flag + exit
    if app.state.flags.lifecycle.reload_pending {
        crate::infra::profiler::log_tool_time(tool_names, pipeline_start.elapsed());
        return;
    }

    push_new_assistant_message(app);
    app.state.streaming_estimated_tokens = 0;

    // Accumulate token stats from intermediate stream before discarding pending_done
    accumulate_pending_token_stats(app);

    // Append per-tick cost row (consumes tick_telemetry populated at stream start)
    super::cost_log::append_cost_tsv(&mut app.state);

    app.save_state_async();

    // Check if any tool requested a sleep (e.g., console send_keys delay)
    if app.state.tool_sleep_until_ms > 0 {
        // Defer everything — main loop will check timer and continue
        app.deferred_tool_sleeping = true;
        app.deferred_tool_sleep_until_ms = app.state.tool_sleep_until_ms;
        app.state.tool_sleep_until_ms = 0; // Clear from state (App owns it now)
        crate::infra::profiler::log_tool_time(tool_names, pipeline_start.elapsed());
        return;
    }

    // Trigger background cache refresh for dirty file panels (non-blocking)
    let _r = trigger_dirty_panel_refresh(&app.state, &app.cache_tx);

    // Check if we need to wait for panels before continuing stream
    if has_dirty_file_panels(&app.state) {
        // Set waiting flag — main loop will check and continue streaming when ready
        app.state.flags.lifecycle.waiting_for_panels = true;
        app.wait_started_ms = now_ms();
    } else {
        // No dirty panels — continue streaming immediately
        crate::app::run::streaming::continue_streaming(app, tx);
    }
    crate::infra::profiler::log_tool_time(tool_names, pipeline_start.elapsed());
}

/// Push a fresh empty assistant message to receive the next stream turn.
fn push_new_assistant_message(app: &mut App) {
    let assistant_id = format!("A{}", app.state.next_assistant_id);
    let assistant_global_uid = format!("UID_{}_A", app.state.global_next_uid);
    app.state.next_assistant_id = app.state.next_assistant_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);
    let new_assistant_msg = Message::new_assistant(assistant_id, assistant_global_uid);
    app.state.messages.push(new_assistant_msg);
}

// Post-execution checks (panels, sleep, question form) live in tool_checks.rs
