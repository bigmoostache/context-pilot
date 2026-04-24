use std::sync::mpsc::Sender;

use crate::app::actions::clean_llm_id_prefix;
use crate::app::panels::now_ms;
use crate::infra::api::StreamEvent;
use crate::infra::tools::execute_tool;
use crate::modules::pre_flight::pre_flight_tool;
use crate::state::persistence::build_message_op;
use crate::state::{Message, MsgKind, MsgStatus, StreamPhase, ToolResultRecord, ToolUseRecord};

use crate::app::run::streaming::{has_dirty_file_panels, trigger_dirty_panel_refresh};
use cp_mod_callback::firing as callback_firing;
use cp_mod_callback::trigger as callback_trigger;
use cp_mod_console::tools::CONSOLE_WAIT_BLOCKING_SENTINEL;
use cp_mod_queue::types::QueueState;

use crate::app::App;
use std::fmt::Write as _;

// ─── Tool pipeline ──────────────────────────────────────────────────────────

/// Accumulate token stats from the intermediate stream into tick/stream/total counters.
pub(crate) const fn accumulate_pending_token_stats(app: &mut App) {
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
}

/// Create and persist a `tool_call` message for a single `ToolUse`.
/// Used for both direct tool calls and queue-flushed replays.
fn save_tool_call_message(app: &mut App, tool: &cp_base::tools::ToolUse) {
    let tool_id = format!("T{}", app.state.next_tool_id);
    let tool_global_uid = format!("UID_{}_T", app.state.global_next_uid);
    app.state.next_tool_id = app.state.next_tool_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

    let tool_msg = Message {
        id: tool_id,
        uid: Some(tool_global_uid),
        role: "assistant".to_string(),
        msg_type: MsgKind::ToolCall,
        content: String::new(),
        content_token_count: 0,
        status: MsgStatus::Full,
        tool_uses: vec![ToolUseRecord { id: tool.id.clone(), name: tool.name.clone(), input: tool.input.clone() }],
        tool_results: Vec::new(),
        input_tokens: 0,
        timestamp_ms: now_ms(),
    };
    app.save_message_async(&tool_msg);
    app.state.messages.push(tool_msg);
}

/// Execute pending tool calls: pre-flight, queue intercept, callbacks, and pipeline resumption.
pub(crate) fn handle_tool_execution(app: &mut App, tx: &Sender<StreamEvent>) {
    if !app.state.flags.stream.phase.is_streaming()
        || app.pending_done.is_none()
        || !app.typewriter.pending_chars.is_empty()
        || app.pending_tools.is_empty()
    {
        return;
    }
    // Don't process new tools while waiting for panels or deferred sleep
    if app.state.flags.lifecycle.waiting_for_panels || app.deferred_tool_sleeping {
        return;
    }
    // Don't process tools while a question form is pending user response
    if app.state.get_ext::<cp_base::ui::question_form::PendingForm>().is_some() {
        return;
    }
    let _guard = crate::profile!("app::tool_exec");

    app.state.flags.ui.dirty = true;
    app.state.flags.stream.phase.transition(StreamPhase::ExecutingTools);
    let mut tools = std::mem::take(&mut app.pending_tools);
    let mut tool_results: Vec<crate::infra::tools::ToolResult> = Vec::new();
    let mut flushed_tools: Vec<super::cleanup::FlushedTool> = Vec::new();

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
    for tool in &tools {
        save_tool_call_message(app, tool);

        let result = if tool.name == "Queue_execute" {
            // Queue flush: execute all queued calls, collect them for pipeline replay
            let (summary_result, flushed) = super::cleanup::execute_queue_flush(tool, &mut app.state);
            flushed_tools = flushed;
            summary_result
        } else {
            // Pre-flight: schema check + module semantic check (ALWAYS runs, queue or not)
            let pf = pre_flight_tool(tool, &app.state, &app.state.active_modules.clone());
            if pf.has_errors() {
                // Hard stop — don't queue, don't execute
                crate::infra::tools::ToolResult::new(tool.id.clone(), pf.format_errors(), true)
            } else {
                // Pre-flight may request queue activation (e.g. destructive operations)
                if pf.activate_queue {
                    let qs = QueueState::get_mut(&mut app.state);
                    if !qs.active {
                        qs.active = true;
                    }
                }

                if QueueState::get(&app.state).active && !QueueState::is_queue_tool(&tool.name) {
                    // Queue intercept: enqueue instead of executing
                    let qs = QueueState::get_mut(&mut app.state);
                    let idx = qs.enqueue(cp_mod_queue::types::QueuedToolCall {
                        index: 0,
                        tool_name: tool.name.clone(),
                        tool_use_id: tool.id.clone(),
                        input: tool.input.clone(),
                        queued_at: now_ms(),
                    });
                    let params = serde_json::to_string(&tool.input).unwrap_or_default();
                    let short = if params.len() > 120 {
                        let mut end = 117;
                        while !params.is_char_boundary(end) {
                            end = end.saturating_sub(1);
                        }
                        format!("{}...", params.get(..end).unwrap_or(""))
                    } else {
                        params
                    };
                    let mut msg = format!("Queued as #{}: {}({})", idx, tool.name, short);
                    if pf.has_warnings() {
                        let _r = write!(msg, "\n{}", pf.format_errors());
                    }
                    crate::infra::tools::ToolResult::new(tool.id.clone(), msg, false)
                } else {
                    // Execute normally
                    let mut result = execute_tool(tool, &mut app.state);
                    if pf.has_warnings() {
                        let _r = write!(result.content, "\n{}", pf.format_errors());
                    }
                    result
                }
            }
        };
        tool_results.push(result);
    }

    // === QUEUE FLUSH REPLAY ===
    // If Queue_execute fired, extend the tools/results vecs with the flushed items.
    // This way callbacks, sentinels, reload checks, and sleep detection see ALL tools —
    // not just the Queue_execute wrapper.
    if !flushed_tools.is_empty() {
        for ft in &flushed_tools {
            save_tool_call_message(app, &ft.tool);
        }
        for ft in flushed_tools {
            tools.push(ft.tool);
            tool_results.push(ft.result);
        }
    }

    // Check if any tool triggered a question form (blocking)
    let has_pending_question = tool_results.iter().any(|r| r.content == "__QUESTION_PENDING__");
    if has_pending_question {
        // Don't create result message or continue streaming yet.
        // The form is active — when user submits/dismisses, check_question_form()
        // will replace the placeholder and resume the pipeline.
        // Store the pending tool results for later resolution.
        app.pending_question_tool_results = Some(tool_results);
        app.save_state_async();
        return;
    }

    // === REVERIE TRIGGER ===
    // Check if any tool result contains a REVERIE_START: sentinel (from optimize_context).
    // Sentinel format: REVERIE_START:<agent_id>\n<context_or_empty>\n<human_readable_msg>
    for tr in &tool_results {
        if let Some(rest) = tr.content.strip_prefix("REVERIE_START:") {
            let mut lines = rest.lines();
            let agent_id = lines.next().unwrap_or("cleaner").to_string();
            let context_line = lines.next().unwrap_or("");
            let context = if context_line.is_empty() { None } else { Some(context_line.to_string()) };
            let _ = crate::app::reverie::trigger::start_manual_reverie(&mut app.state, agent_id, context);
            break;
        }
    }

    // === CALLBACK TRIGGER ===
    // After all tools executed, check if any file edits match active callbacks.
    // Only collect files from SUCCESSFUL Edit/Write tools (skip failed ones).
    let successful_tools: Vec<_> =
        tools.iter().zip(tool_results.iter()).filter(|(_, r)| !r.is_error).map(|(t, _)| t.clone()).collect();
    let changed_files = callback_trigger::collect_changed_files(&successful_tools);
    if !changed_files.is_empty() {
        let (matched, skip_warnings) = callback_trigger::match_callbacks(&app.state, &changed_files);

        // Inject skip_callbacks warnings into tool results so the AI sees them
        if !skip_warnings.is_empty() {
            let warning_note = format!("\n\n[skip_callbacks warnings: {}]", skip_warnings.join("; "));
            for tr in tool_results.iter_mut().rev() {
                if tr.tool_name == "Edit" || tr.tool_name == "Write" {
                    tr.content.push_str(&warning_note);
                    if let Some(ref mut disp) = tr.display {
                        disp.push_str(&warning_note);
                    }
                    break;
                }
            }
        }

        if !matched.is_empty() {
            let (blocking_cbs, async_cbs) = callback_trigger::partition_callbacks(matched);

            // Fire non-blocking callbacks immediately (they run async via watchers)
            if !async_cbs.is_empty() {
                let summaries = callback_firing::fire_async_callbacks(&mut app.state, &async_cbs);
                // Append compact callback summary to the last Edit/Write tool result
                if !summaries.is_empty() {
                    let note = format!("\nCallbacks:\n{}", summaries.join("\n"));
                    // Find the last Edit/Write tool result and append the note
                    for tr in tool_results.iter_mut().rev() {
                        if tr.tool_name == "Edit" || tr.tool_name == "Write" {
                            tr.content.push_str(&note);
                            if let Some(ref mut disp) = tr.display {
                                disp.push_str(&note);
                            }
                            break;
                        }
                    }
                }
            }

            // Fire blocking callbacks — these hold the pipeline until completion.
            // CONSTRAINT: each tool_call must have exactly 1 tool_result.
            // We do NOT create a synthetic tool_use/tool_result pair.
            // Instead, we tag the last Edit/Write tool result with a sentinel
            // and defer all results until the callback watcher completes.
            if !blocking_cbs.is_empty() {
                // Generate a unique sentinel ID for the blocking watcher
                let sentinel_id = format!("cb_block_{}", app.state.next_tool_id);
                app.state.next_tool_id = app.state.next_tool_id.saturating_add(1);

                let _summaries = callback_firing::fire_blocking_callbacks(&mut app.state, &blocking_cbs, &sentinel_id);

                // Tag the last Edit/Write tool result with sentinel so pipeline knows to wait.
                // Store original content so we can reconstruct: original + callback output.
                for tr in tool_results.iter_mut().rev() {
                    if tr.tool_name == "Edit" || tr.tool_name == "Write" {
                        tr.content = format!("{}{}{}", CONSOLE_WAIT_BLOCKING_SENTINEL, sentinel_id, tr.content,);
                        break;
                    }
                }
            }
        }
    }

    // Check if any tool triggered a console blocking wait
    let has_console_wait = tool_results.iter().any(|r| r.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL));
    if has_console_wait {
        app.pending_console_wait_tool_results = Some(tool_results);
        app.save_state_async();
        return;
    }

    // Create tool result message
    let result_id = format!("R{}", app.state.next_result_id);
    let result_global_uid = format!("UID_{}_R", app.state.global_next_uid);
    app.state.next_result_id = app.state.next_result_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);
    let tool_result_records: Vec<ToolResultRecord> = tool_results
        .iter()
        .zip(tools.iter())
        .map(|(r, t)| ToolResultRecord {
            tool_use_id: r.tool_use_id.clone(),
            content: r.content.clone(),
            display: r.display.clone(),
            is_error: r.is_error,
            tool_name: t.name.clone(),
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

    // Check if reload was requested — main loop will handle flag + exit
    if app.state.flags.lifecycle.reload_pending {
        return;
    }

    // Create new assistant message
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

    // Accumulate token stats from intermediate stream before discarding pending_done
    accumulate_pending_token_stats(app);

    app.save_state_async();

    // Check if any tool requested a sleep (e.g., console send_keys delay)
    if app.state.tool_sleep_until_ms > 0 {
        // Defer everything — main loop will check timer and continue
        app.deferred_tool_sleeping = true;
        app.deferred_tool_sleep_until_ms = app.state.tool_sleep_until_ms;
        app.state.tool_sleep_until_ms = 0; // Clear from state (App owns it now)
        return;
    }

    // Trigger background cache refresh for dirty file panels (non-blocking)
    let _ = trigger_dirty_panel_refresh(&app.state, &app.cache_tx);

    // Check if we need to wait for panels before continuing stream
    if has_dirty_file_panels(&app.state) {
        // Set waiting flag — main loop will check and continue streaming when ready
        app.state.flags.lifecycle.waiting_for_panels = true;
        app.wait_started_ms = now_ms();
    } else {
        // No dirty panels — continue streaming immediately
        crate::app::run::streaming::continue_streaming(app, tx);
    }
}

// Post-execution checks (panels, sleep, question form) live in tool_checks.rs
