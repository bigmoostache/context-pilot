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
    while let Ok(evt) = rx.try_recv() {
        if !app.state.flags.stream.phase.is_streaming() {
            continue;
        }
        app.state.flags.ui.dirty = true;
        match evt {
            StreamEvent::Chunk(text) => {
                app.typewriter.add_chunk(&text);
            }
            StreamEvent::ToolProgress { name, input_so_far } => {
                // Notify modules of streaming tool progress (e.g. typing indicators)
                for module in crate::modules::all_modules() {
                    module.on_tool_progress(&name, &input_so_far, &mut app.state);
                }
                app.state.streaming_tool = Some(crate::state::StreamingTool { name, input_so_far });
            }
            StreamEvent::ToolUse(tool) => {
                // Notify modules that a tool call completed (e.g. clear typing)
                for module in crate::modules::all_modules() {
                    module.on_tool_complete(&tool.name, &mut app.state);
                }
                app.state.streaming_tool = None;
                app.pending_tools.push(tool);
            }
            StreamEvent::Done { input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason } => {
                app.typewriter.mark_done();
                app.state.streaming_tool = None;
                app.pending_done =
                    Some((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason));
                // API call succeeded — reset retry counter immediately at tick level
                app.state.api_retry_count = 0;
            }
            StreamEvent::Error(e) => {
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
                    spine.config.consecutive_continuation_errors =
                        spine.config.consecutive_continuation_errors.saturating_add(1);
                    spine.config.last_continuation_error_ms = Some(crate::app::panels::now_ms());
                    let _action = apply_action(&mut app.state, Action::StreamError(e));
                }
            }
        }
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
    if let Some(rx) = &app.api_check_rx
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
    if !app.state.flags.stream.phase.is_streaming() {
        return;
    }
    // Don't finalize while waiting for panels or deferred sleep —
    // pending_done is still Some from the intermediate stream, and
    // continue_streaming will clear it when the deferred state resolves.
    if app.state.flags.lifecycle.waiting_for_panels || app.deferred_tool_sleeping {
        return;
    }
    // Don't finalize while a question form is pending user response
    if app.pending_question_tool_results.is_some() {
        return;
    }
    // Don't finalize while a console blocking wait is pending
    if app.pending_console_wait_tool_results.is_some() {
        return;
    }

    if let Some((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, ref stop_reason)) = app.pending_done
        && app.typewriter.pending_chars.is_empty()
        && app.pending_tools.is_empty()
    {
        app.state.flags.ui.dirty = true;
        let stop_reason = stop_reason.clone();
        match apply_action(
            &mut app.state,
            Action::StreamDone { input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason },
        ) {
            ActionResult::SaveMessage(id) => {
                if let Some(msg) = app.state.messages.iter().find(|m| m.id == id) {
                    app.save_message_async(msg);
                }
                app.save_state_async();
            }
            ActionResult::Save => app.save_state_async(),
            ActionResult::Nothing | ActionResult::StopStream | ActionResult::StartApiCheck => {}
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

        // Check if any chat rooms are still awaiting a response from the AI.
        // If so, fire a Spine notification so the next auto-continuation addresses them.
        check_chat_report_here(&mut app.state);

        app.typewriter.reset();
        app.pending_done = None;
    }
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

/// Fire a Spine notification if any chat rooms still await a response.
///
/// Called after stream completion so the AI remembers to reply in rooms
/// where messages arrived during this stream or a previous one.
fn check_chat_report_here(state: &mut State) {
    use cp_mod_chat::types::ChatState;
    use cp_mod_spine::types::{NotificationType, SpineState};

    // Bail early if chat module isn't active (no ChatState in TypeMap)
    let report_rooms: Vec<String> = {
        let Some(cs) = state.get_ext::<ChatState>() else {
            return;
        };
        if cs.report_here.is_empty() {
            return;
        }
        cs.report_here.iter().cloned().collect()
    };

    // Resolve room IDs to display names with C-refs for the notification
    let cs = ChatState::get(state);
    let room_names: Vec<String> = report_rooms
        .iter()
        .map(|rid| {
            let name =
                cs.rooms.iter().find(|r| &r.room_id == rid).map_or_else(|| rid.clone(), |r| r.display_name.clone());
            // Prepend C-ref if assigned (e.g. "C1 MyBots")
            cs.room_id_to_ref.get(rid).map_or_else(|| name.clone(), |cref| format!("{cref} {name}"))
        })
        .collect();

    // Deduplicate: don't fire if an unprocessed report_here notification exists
    let already =
        SpineState::get(state).notifications.iter().any(|n| !n.is_processed() && n.source == "chat_report_here");
    if already {
        return;
    }

    let content = format!(
        "You haven't responded in {} room(s): {}. Please reply or use report_later_here=true if you plan to follow up later.",
        room_names.len(),
        room_names.join(", ")
    );

    let _id = SpineState::create_notification(state, NotificationType::Custom, "chat_report_here".to_string(), content);
}
