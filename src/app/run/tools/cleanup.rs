use std::sync::mpsc::Sender;

use crate::app::panels::now_ms;
use crate::infra::api::StreamEvent;
use crate::infra::tools::execute_tool;
use crate::state::{Message, MsgKind, MsgStatus, State, ToolResultRecord};

use cp_base::state::watchers::WatcherRegistry;
use cp_mod_console::tools::CONSOLE_WAIT_BLOCKING_SENTINEL;
use cp_mod_queue::types::QueueState;
use cp_mod_spine::types::{NotificationType, SpineState};

use crate::app::App;
use std::fmt::Write as _;

/// Non-blocking check: poll `WatcherRegistry` for satisfied conditions.
/// - Blocking watchers: replace sentinel tool results and resume pipeline.
/// - Async watchers: create spine notifications.
pub(crate) fn check_watchers(app: &mut App, tx: &Sender<StreamEvent>) {
    // Take the registry out of state to avoid borrow conflict
    // (poll_all needs &mut registry + &state simultaneously)
    let mut registry = match app.state.module_data.remove(&std::any::TypeId::of::<WatcherRegistry>()) {
        Some(boxed) => match boxed.downcast::<WatcherRegistry>() {
            Ok(r) => *r,
            Err(boxed) => {
                let _r = app.state.module_data.insert(std::any::TypeId::of::<WatcherRegistry>(), boxed);
                return;
            }
        },
        None => return,
    };

    let (blocking_results, mut async_results) = registry.poll_all(&app.state);

    // Put registry back
    app.state.set_ext(registry);

    // --- Async completions → spine notifications ---
    if !async_results.is_empty() {
        // Handle deferred panel creation FIRST (so we have panel IDs for notifications)
        for result in &mut async_results {
            if let Some(ref dp) = result.create_panel {
                let panel_id = app.state.next_available_context_id();
                let uid = format!("UID_{}_P", app.state.global_next_uid);
                app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

                let mut ctx = crate::state::make_default_entry(
                    &panel_id,
                    cp_base::state::context::Kind::new(cp_base::state::context::Kind::CONSOLE),
                    &dp.display_name,
                    true,
                );
                ctx.uid = Some(uid);
                ctx.set_meta("console_name", &dp.session_key);
                ctx.set_meta("console_command", &dp.command);
                ctx.set_meta("console_description", &dp.description);
                ctx.set_meta("callback_id", &dp.callback_id);
                ctx.set_meta("callback_name", &dp.callback_name);
                if let Some(ref dir) = dp.cwd {
                    ctx.set_meta("console_cwd", dir);
                }
                app.state.context.push(ctx);
                // Enrich the result description with the panel reference
                // Panel is already populated and exited — no console_wait needed.
                result.description.push_str(" → see ");
                result.description.push_str(&panel_id);
                result.description.push_str(" (already loaded, read it directly)");
            }
            // Auto-close panels for watchers that request it
            if result.close_panel
                && let Some(ref panel_id) = result.panel_id
            {
                if let Some(ctx) = app.state.context.iter().find(|c| c.id == *panel_id)
                    && let Some(name) = ctx.get_meta::<String>("console_name")
                {
                    cp_mod_console::types::ConsoleState::kill_session(&mut app.state, &name);
                }
                app.state.context.retain(|c| c.id != *panel_id);
            }
        }

        // Now create notifications (after panel creation, so descriptions include panel refs)
        for result in &async_results {
            let nid = SpineState::create_notification(
                &mut app.state,
                NotificationType::Custom,
                "watcher".to_string(),
                result.description.clone(),
            );
            if result.processed_already {
                let _r = SpineState::mark_notification_processed(&mut app.state, &nid);
            }
        }

        app.save_state_async();
    }

    // --- Blocking sentinel replacement ---
    if app.pending_console_wait_tool_results.is_none() || blocking_results.is_empty() {
        return;
    }

    // Accumulate partial blocking results into App-level storage.
    // Multiple blocking callbacks share one sentinel_id but complete at different times.
    // We must wait for ALL of them before resuming the pipeline.
    app.accumulated_blocking_results.extend(blocking_results);

    // Check if there are STILL blocking watchers pending in the registry.
    // If so, don't resume yet — more results are coming.
    let watcher_reg = WatcherRegistry::get(&app.state);
    if watcher_reg.has_blocking_watchers() {
        return;
    }

    // All blocking watchers done — merge accumulated results and resume pipeline.
    let mut merged_blocking = std::mem::take(&mut app.accumulated_blocking_results);

    let Some(mut tool_results) = app.pending_console_wait_tool_results.take() else {
        return;
    };

    // Handle deferred panel creation FIRST — so descriptions include panel IDs
    // before we copy them into tool results during sentinel replacement.
    for result in &mut merged_blocking {
        if let Some(ref dp) = result.create_panel {
            let panel_id = app.state.next_available_context_id();
            let uid = format!("UID_{}_P", app.state.global_next_uid);
            app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

            let mut ctx = crate::state::make_default_entry(
                &panel_id,
                cp_base::state::context::Kind::new(cp_base::state::context::Kind::CONSOLE),
                &dp.display_name,
                true,
            );
            ctx.uid = Some(uid);
            ctx.set_meta("console_name", &dp.session_key);
            ctx.set_meta("console_command", &dp.command);
            ctx.set_meta("console_description", &dp.description);
            ctx.set_meta("callback_id", &dp.callback_id);
            ctx.set_meta("callback_name", &dp.callback_name);
            if let Some(ref dir) = dp.cwd {
                ctx.set_meta("console_cwd", dir);
            }
            app.state.context.push(ctx);
            // Point the LLM at exactly which panel to read for the error
            // Panel is already populated and exited — no console_wait needed.
            result.description.push_str(" → see ");
            result.description.push_str(&panel_id);
            result.description.push_str(" (already loaded, read it directly)");
        }
    }

    // Replace sentinels with real results (descriptions now include panel IDs)
    for tr in &mut tool_results {
        if tr.content == CONSOLE_WAIT_BLOCKING_SENTINEL {
            // Console wait sentinel: replace entirely with watcher result
            if let Some(result) = merged_blocking.iter().find(|r| r.tool_use_id.as_deref() == Some(&tr.tool_use_id)) {
                tr.content = result.description.clone();
            }
        } else if tr.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL) {
            // Callback blocking sentinel: format is "SENTINEL{sentinel_id}{original_content}"
            // Extract sentinel_id and original content, then merge with callback result
            let after_sentinel = &tr.content.get(CONSOLE_WAIT_BLOCKING_SENTINEL.len()..).unwrap_or("");
            // Find matching watcher result by sentinel_id prefix
            let matched_result = merged_blocking
                .iter()
                .find(|r| r.tool_use_id.as_ref().is_some_and(|tid| after_sentinel.starts_with(tid.as_str())));
            if let Some(result) = matched_result
                && let Some(sentinel_id) = result.tool_use_id.as_ref()
            {
                let original_content = &after_sentinel.get(sentinel_id.len()..).unwrap_or("");
                // Collect ALL blocking results for this sentinel (multiple callbacks)
                let all_matched: Vec<&str> = merged_blocking
                    .iter()
                    .filter(|r| r.tool_use_id.as_deref() == Some(sentinel_id.as_str()))
                    .map(|r| r.description.as_str())
                    .filter(|d| !d.is_empty())
                    .collect();
                let merged_descriptions = all_matched.join("\n");
                // Append to existing Callbacks block if present, else create new one
                if original_content.contains("\nCallbacks:\n") {
                    tr.content = format!("{original_content}\n{merged_descriptions}");
                } else {
                    tr.content = format!("{original_content}\nCallbacks:\n{merged_descriptions}");
                }
            }
        }
    }

    // Check if any sentinels remain unresolved (multiple blocking waits in one batch)
    let still_pending = tool_results.iter().any(|r| r.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL));
    if still_pending {
        // Safety valve: if no blocking watchers remain but sentinels are unresolved,
        // force-resolve them to prevent infinite pipeline stall. This can happen when
        // a watcher fires with a stale tool_use_id that doesn't match any pending result.
        let safety_reg = WatcherRegistry::get(&app.state);
        if safety_reg.has_blocking_watchers() {
            app.pending_console_wait_tool_results = Some(tool_results);
            return;
        }

        for tr in &mut tool_results {
            if tr.content == CONSOLE_WAIT_BLOCKING_SENTINEL {
                tr.content = "Console wait result unavailable (watcher expired or was interrupted)".to_string();
            } else if tr.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL) {
                // Callback sentinel: extract original content after sentinel+id prefix
                let after = &tr.content.get(CONSOLE_WAIT_BLOCKING_SENTINEL.len()..).unwrap_or("");
                // Try to find where the original content starts (after sentinel_id)
                tr.content = format!("Callback result unavailable (timeout). Original: {after}");
            }
        }
    }

    // All resolved — resume normal pipeline: create result message + continue streaming
    let result_id = format!("R{}", app.state.next_result_id);
    let result_global_uid = format!("UID_{}_R", app.state.global_next_uid);
    app.state.next_result_id = app.state.next_result_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);
    let tool_result_records: Vec<ToolResultRecord> = tool_results
        .iter()
        .map(|r| ToolResultRecord {
            tool_use_id: r.tool_use_id.clone(),
            content: r.content.clone(),
            display: r.display.clone(),
            is_error: r.is_error,
            tool_name: r.tool_name.clone(),
        })
        .collect();
    let result_msg = Message {
        id: result_id,
        uid: Some(result_global_uid),
        role: "user".to_string(),
        msg_type: MsgKind::ToolResult,
        content: String::new(),
        content_token_count: 0,

        status: MsgStatus::Full,
        tool_uses: Vec::new(),
        tool_results: tool_result_records,
        input_tokens: 0,
        timestamp_ms: now_ms(),
    };
    app.save_message_async(&result_msg);
    app.state.messages.push(result_msg);

    if app.state.flags.lifecycle.reload_pending {
        return;
    }

    // Create new assistant message for continued streaming
    let assistant_id = format!("A{}", app.state.next_assistant_id);
    let assistant_global_uid = format!("UID_{}_A", app.state.global_next_uid);
    app.state.next_assistant_id = app.state.next_assistant_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);
    let new_assistant_msg = Message {
        id: assistant_id,
        uid: Some(assistant_global_uid),
        role: "assistant".to_string(),
        msg_type: MsgKind::TextMessage,
        content: String::new(),
        content_token_count: 0,

        status: MsgStatus::Full,
        tool_uses: Vec::new(),
        tool_results: Vec::new(),
        input_tokens: 0,
        timestamp_ms: now_ms(),
    };
    app.state.messages.push(new_assistant_msg);

    app.state.streaming_estimated_tokens = 0;

    // Accumulate token stats from intermediate stream
    if let Some((_, output_tokens, cache_hit_tokens, cache_miss_tokens, _)) = app.pending_done {
        app.state.tick_cache_hit_tokens = cache_hit_tokens;
        app.state.tick_cache_miss_tokens = cache_miss_tokens;
        app.state.tick_output_tokens = output_tokens;
        app.state.stream_cache_hit_tokens = app.state.stream_cache_hit_tokens.saturating_add(cache_hit_tokens);
        app.state.stream_cache_miss_tokens = app.state.stream_cache_miss_tokens.saturating_add(cache_miss_tokens);
        app.state.stream_output_tokens = app.state.stream_output_tokens.saturating_add(output_tokens);
        app.state.cache_hit_tokens = app.state.cache_hit_tokens.saturating_add(cache_hit_tokens);
        app.state.cache_miss_tokens = app.state.cache_miss_tokens.saturating_add(cache_miss_tokens);
        app.state.total_output_tokens = app.state.total_output_tokens.saturating_add(output_tokens);
    }

    app.save_state_async();
    app.state.flags.ui.dirty = true;

    let _ = crate::app::run::streaming::trigger_dirty_panel_refresh(&app.state, &app.cache_tx);
    if crate::app::run::streaming::has_dirty_file_panels(&app.state) {
        app.state.flags.lifecycle.waiting_for_panels = true;
        app.wait_started_ms = now_ms();
    } else {
        crate::app::run::streaming::continue_streaming(app, tx);
    }
}

/// When the user interrupts streaming (Esc), any pending blocking tool calls
/// (`console_wait`, `ask_user_question`, or tools mid-execution) have their
/// `tool_use` messages already saved but no matching `tool_result`. This creates
/// orphaned `tool_use` blocks that cause API 400 errors on the next stream.
///
/// This method creates fake `tool_result` messages for all pending tools so
/// every `tool_use` is properly paired.
pub(crate) fn flush_pending_tool_results_as_interrupted(app: &mut App) {
    let interrupted_msg = "Tool execution interrupted by user.";

    // Collect all pending tool results from both blocking paths
    let mut all_pending: Vec<crate::infra::tools::ToolResult> = Vec::new();

    if let Some(results) = app.pending_console_wait_tool_results.take() {
        all_pending.extend(results);
    }
    if let Some(results) = app.pending_question_tool_results.take() {
        all_pending.extend(results);
    }

    // Also clean up the question form state if it was pending
    drop(app.state.module_data.remove(&std::any::TypeId::of::<cp_base::ui::question_form::PendingForm>()));

    // Clear any accumulated blocking results from partial callback completions
    app.accumulated_blocking_results.clear();

    // Scuttle stale blocking watchers whose tool_use_ids match the interrupted results.
    // Without this, interrupted watchers linger in the registry and fire later with
    // stale IDs, causing sentinel replacement to fail permanently on the next stream.
    {
        let stale_ids: Vec<String> = all_pending
            .iter()
            .filter(|r| r.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL))
            .map(|r| r.tool_use_id.clone())
            .collect();
        if !stale_ids.is_empty() {
            let registry = WatcherRegistry::get_mut(&mut app.state);
            registry.watchers.retain(|w| w.tool_use_id().is_none_or(|tid| !stale_ids.contains(&tid.to_string())));
        }
    }

    if all_pending.is_empty() {
        return;
    }

    // Create a tool_result message pairing each pending tool_use
    let result_id = format!("R{}", app.state.next_result_id);
    let result_global_uid = format!("UID_{}_R", app.state.global_next_uid);
    app.state.next_result_id = app.state.next_result_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

    let tool_result_records: Vec<ToolResultRecord> = all_pending
        .iter()
        .map(|r| {
            // Strip any callback blocking sentinel prefix from content
            let content = interrupted_msg.to_string();
            ToolResultRecord {
                tool_use_id: r.tool_use_id.clone(),
                content,
                display: None,
                is_error: true,
                tool_name: r.tool_name.clone(),
            }
        })
        .collect();

    let result_msg = Message {
        id: result_id,
        uid: Some(result_global_uid),
        role: "user".to_string(),
        msg_type: MsgKind::ToolResult,
        content: String::new(),
        content_token_count: 0,

        status: MsgStatus::Full,
        tool_uses: Vec::new(),
        tool_results: tool_result_records,
        input_tokens: 0,
        timestamp_ms: now_ms(),
    };
    app.save_message_async(&result_msg);
    app.state.messages.push(result_msg);
}

// ─── Queue flush (called from tool_pipeline.rs) ─────────────────────────────

/// Flushed tool execution pair: the original `ToolUse` and its result.
pub(crate) struct FlushedTool {
    /// The original tool-use request that was dequeued and executed.
    pub tool: cp_base::tools::ToolUse,
    /// The execution result for this tool call.
    pub result: crate::infra::tools::ToolResult,
}

/// Execute all queued tool calls in order.
/// Returns (`summary_result`, `flushed_tools`) so the pipeline can run callbacks/sentinels
/// on the individual tools — not just the `Queue_execute` wrapper.
pub(crate) fn execute_queue_flush(
    tool: &cp_base::tools::ToolUse,
    state: &mut State,
) -> (crate::infra::tools::ToolResult, Vec<FlushedTool>) {
    let qs = QueueState::get_mut(state);
    if qs.queued_calls.is_empty() {
        return (
            crate::infra::tools::ToolResult::new(
                tool.id.clone(),
                "Queue is empty — nothing to execute.".to_string(),
                false,
            ),
            Vec::new(),
        );
    }
    let calls = qs.flush();
    qs.active = false;

    let mut summary = format!("Executed {} queued action(s):\n", calls.len());
    let mut flushed = Vec::with_capacity(calls.len());

    for call in &calls {
        // Generate a fresh tool_use_id to avoid collision with the intercept-time message.
        // The original id was already used in the "Queued as #N" tool_result at intercept time.
        let fresh_id = format!("flush_{}_{}", call.index, call.tool_use_id);
        let queued_tool =
            cp_base::tools::ToolUse { id: fresh_id, name: call.tool_name.clone(), input: call.input.clone() };
        let result = execute_tool(&queued_tool, state);
        let status = if result.is_error { "ERROR" } else { "ok" };
        let short = if result.content.len() > 100 {
            let end = result.content.floor_char_boundary(97);
            format!("{}...", result.content.get(..end).unwrap_or(""))
        } else {
            result.content.clone()
        };
        let _r = writeln!(summary, "{}. {} → {} ({})", call.index, call.tool_name, status, short);
        flushed.push(FlushedTool { tool: queued_tool, result });
    }

    (crate::infra::tools::ToolResult::new(tool.id.clone(), summary, false), flushed)
}
