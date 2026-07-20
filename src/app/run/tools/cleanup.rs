use std::sync::mpsc::Sender;

use crate::app::panels::now_ms;
use crate::infra::api::StreamEvent;
use crate::state::{Message, ToolResultRecord};

use cp_base::state::watchers::{ASYNC_ERROR_PREFIX, WatcherRegistry};
use cp_mod_console::tools::CONSOLE_WAIT_BLOCKING_SENTINEL;
use cp_mod_spine::types::{NotificationType, SpineState};

use crate::app::App;

/// Create a console panel for a watcher result's deferred `create_panel`, and
/// append a "→ see {panel}" pointer to the result description. No-op when absent.
fn create_console_panel_for(app: &mut App, result: &mut cp_base::state::watchers::carriers::WatcherResult) {
    let Some(dp) = &(result.create_panel) else { return };
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
    if let Some(dir) = &(dp.cwd) {
        ctx.set_meta("console_cwd", dir);
    }
    app.state.context.push(ctx);
    // Panel is already populated and exited — no console_wait needed.
    result.description.push_str(" \u{2192} see ");
    result.description.push_str(&panel_id);
    result.description.push_str(" (already loaded, read it directly)");
}

/// Create a generic dyn panel for a watcher result's deferred `create_dyn_panel`,
/// substituting the panel-ID placeholder in the description. No-op when absent.
fn create_dyn_panel_for(app: &mut App, result: &mut cp_base::state::watchers::carriers::WatcherResult) {
    let Some(dp) = &(result.create_dyn_panel) else { return };
    let panel_id = app.state.next_available_context_id();
    let uid = format!("UID_{}_P", app.state.global_next_uid);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

    let mut ctx = crate::state::make_default_entry(
        &panel_id,
        cp_base::state::context::Kind::new(&dp.context_type),
        &dp.display_name,
        true,
    );
    ctx.uid = Some(uid);
    for (key, value) in &dp.metadata {
        ctx.set_meta(key, value);
    }
    if let Some(content) = &(dp.content) {
        ctx.cached_content = Some(content.clone());
        ctx.token_count = cp_base::state::context::estimate_tokens(content);
        ctx.full_token_count = ctx.token_count;
        ctx.total_pages = cp_base::state::context::compute_total_pages(ctx.token_count);
        ctx.cache_deprecated = false;
    }
    app.state.context.push(ctx);
    result.description = result.description.replace(cp_base::state::watchers::DYN_PANEL_ID_PLACEHOLDER, &panel_id);
}

/// Kill console sessions tied to inline (panel-less) watcher results and remove
/// their log files. Runs over both blocking and async result sets.
fn cleanup_inline_sessions(
    app: &mut App,
    blocking: &[cp_base::state::watchers::carriers::WatcherResult],
    async_res: &[cp_base::state::watchers::carriers::WatcherResult],
) {
    for result in blocking.iter().chain(async_res.iter()) {
        let Some(name) = &(result.kill_session) else { continue };
        let log_path = {
            let cs = cp_mod_console::types::ConsoleState::get(&app.state);
            cs.sessions.get(name).map(|h| h.log_path.clone()).unwrap_or_default()
        };
        cp_mod_console::types::ConsoleState::kill_session(&mut app.state, name);
        {
            let cs = cp_mod_console::types::ConsoleState::get_mut(&mut app.state);
            drop(cs.sessions.remove(name));
        }
        if !log_path.is_empty() {
            let _: Option<()> = std::fs::remove_file(&log_path).ok();
        }
    }
}

/// Process async (non-blocking) watcher completions: create their panels, then
/// emit spine notifications (after panel creation so descriptions carry refs).
fn process_async_completions(app: &mut App, async_results: &mut [cp_base::state::watchers::carriers::WatcherResult]) {
    for result in async_results.iter_mut() {
        create_console_panel_for(app, result);
        create_dyn_panel_for(app, result);
        // Auto-close panels for watchers that request it
        if result.close_panel
            && let Some(panel_id) = &result.panel_id
        {
            if let Some(ctx) = app.state.context.iter().find(|c| c.id == *panel_id)
                && let Some(name) = ctx.get_meta::<String>("console_name")
            {
                cp_mod_console::types::ConsoleState::kill_session(&mut app.state, &name);
            }
            app.state.context.retain(|c| c.id != *panel_id);
        }
    }

    for result in async_results.iter() {
        let nid = SpineState::create_notification(
            &mut app.state,
            NotificationType::Custom,
            "watcher".to_owned(),
            result.description.clone(),
        );
        if result.processed_already {
            let _r = SpineState::mark_notification_processed(&mut app.state, &nid);
        }
    }

    app.save_state_async();
}

/// Replace one console-wait sentinel `tr` with its matching watcher result.
/// Async tools encode error status via `ASYNC_ERROR_PREFIX` in the description
/// (`WatcherResult` can't carry `is_error` due to `struct_excessive_bools` forbid).
fn apply_console_wait_result(
    tr: &mut crate::infra::tools::ToolResult,
    merged_blocking: &[cp_base::state::watchers::carriers::WatcherResult],
) {
    if let Some(result) = merged_blocking.iter().find(|r| r.tool_use_id.as_deref() == Some(&tr.tool_use_id)) {
        if let Some(stripped) = result.description.strip_prefix(ASYNC_ERROR_PREFIX) {
            tr.content = stripped.to_owned();
            tr.is_error = true;
        } else {
            tr.content = result.description.clone();
        }
    }
}

/// Merge callback blocking-sentinel content ("SENTINEL{id}{original}") with all
/// matching callback watcher results, appending a "Callbacks:" block.
fn apply_callback_result(
    tr: &mut crate::infra::tools::ToolResult,
    merged_blocking: &[cp_base::state::watchers::carriers::WatcherResult],
) {
    let after_sentinel = &tr.content.get(CONSOLE_WAIT_BLOCKING_SENTINEL.len()..).unwrap_or("");
    let matched_result = merged_blocking
        .iter()
        .find(|r| r.tool_use_id.as_ref().is_some_and(|tid| after_sentinel.starts_with(tid.as_str())));
    let Some(result) = matched_result else { return };
    let Some(sentinel_id) = result.tool_use_id.as_ref() else { return };
    let original_content = &after_sentinel.get(sentinel_id.len()..).unwrap_or("");
    // Collect ALL blocking results for this sentinel (multiple callbacks share one id)
    let all_matched: Vec<&str> = merged_blocking
        .iter()
        .filter(|r| r.tool_use_id.as_deref() == Some(sentinel_id.as_str()))
        .map(|r| r.description.as_str())
        .filter(|d| !d.is_empty())
        .collect();
    let merged_descriptions = all_matched.join("\n");
    if original_content.contains("\nCallbacks:\n") {
        tr.content = format!("{original_content}\n{merged_descriptions}");
    } else {
        tr.content = format!("{original_content}\nCallbacks:\n{merged_descriptions}");
    }
    if let Some(ref mut disp) = tr.display {
        disp.push_str("\nCallbacks:\n");
        disp.push_str(&merged_descriptions);
    }
}

/// Force-resolve any sentinels still unresolved after matching (stale `tool_use_id`),
/// substituting a "result unavailable" message so the pipeline never stalls.
fn force_resolve_stragglers(tool_results: &mut [crate::infra::tools::ToolResult]) {
    for tr in tool_results {
        if tr.content == CONSOLE_WAIT_BLOCKING_SENTINEL {
            "Console wait result unavailable (watcher expired or was interrupted)".clone_into(&mut tr.content);
        } else if tr.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL) {
            let after = &tr.content.get(CONSOLE_WAIT_BLOCKING_SENTINEL.len()..).unwrap_or("");
            tr.content = format!("Callback result unavailable (timeout). Original: {after}");
        } else {
            // Not a sentinel — already a real result, leave untouched.
        }
    }
}

/// Replace all blocking sentinels in `tool_results` with their watcher results.
/// Returns `true` when unresolved sentinels remain AND blocking watchers are still
/// pending — the caller stashed the results and must return early.
fn replace_blocking_sentinels(
    app: &mut App,
    tool_results: &mut Vec<crate::infra::tools::ToolResult>,
    merged_blocking: &[cp_base::state::watchers::carriers::WatcherResult],
) -> bool {
    for tr in tool_results.iter_mut() {
        if tr.content == CONSOLE_WAIT_BLOCKING_SENTINEL {
            apply_console_wait_result(tr, merged_blocking);
        } else if tr.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL) {
            apply_callback_result(tr, merged_blocking);
        } else {
            // Non-sentinel result — nothing to replace.
        }
    }

    let still_pending = tool_results.iter().any(|r| r.content.starts_with(CONSOLE_WAIT_BLOCKING_SENTINEL));
    if still_pending {
        // A stale tool_use_id left a sentinel unmatched. If real watchers remain,
        // stash and wait; otherwise force-resolve to avoid an infinite stall.
        if WatcherRegistry::get(&app.state).has_blocking_watchers() {
            app.pending_console_wait_tool_results = Some(std::mem::take(tool_results));
            return true;
        }
        force_resolve_stragglers(tool_results);
    }
    false
}

/// Non-blocking check: poll `WatcherRegistry` for satisfied conditions.
/// - Blocking watchers: replace sentinel tool results and resume pipeline.
/// - Async watchers: create spine notifications.
pub(crate) fn check_watchers(app: &mut App, tx: &Sender<StreamEvent>) {
    let _fg = cp_base::flame!("watchers");
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

    // --- Session cleanup for inline easy_bash results (no panel to close) ---
    cleanup_inline_sessions(app, &blocking_results, &async_results);

    // --- Async completions → spine notifications (panels first, then notify) ---
    if !async_results.is_empty() {
        process_async_completions(app, &mut async_results);
    }

    // --- Blocking sentinel replacement ---
    if app.pending_console_wait_tool_results.is_none() {
        return;
    }

    // If no new blocking results came in this poll cycle, check if any blocking
    // watchers still exist. If none remain (all spawned callbacks completed, timed out,
    // or ALL failed to spawn), fall through to resume the pipeline.
    // Without this, a spawn failure (e.g., "too many open files") that prevents watcher
    // registration causes an infinite stall — no watcher ever fires, blocking_results
    // stays empty forever, and the pipeline never resumes.
    if blocking_results.is_empty() {
        let watcher_reg = WatcherRegistry::get(&app.state);
        if watcher_reg.has_blocking_watchers() {
            return; // Still waiting for real watchers to complete
        }
        // No blocking watchers remain — fall through to resolve sentinels and resume
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
        create_console_panel_for(app, result);
        create_dyn_panel_for(app, result);
    }

    // Replace sentinels with real watcher results, then force-resolve stragglers.
    // Returns true if it stashed results and the caller must return early.
    if replace_blocking_sentinels(app, &mut tool_results, &merged_blocking) {
        return;
    }

    // Break tempo, build result + assistant messages, resume streaming.
    resume_pipeline_after_blocking(app, tx, &tool_results, &merged_blocking);
}

/// After all blocking watchers resolve: apply their deferred tempo break, emit the
/// paired `ToolResult` message + a fresh assistant message, accumulate token/cost
/// stats from the intermediate stream, then resume streaming (or wait on dirty panels).
fn resume_pipeline_after_blocking(
    app: &mut App,
    tx: &Sender<StreamEvent>,
    tool_results: &[crate::infra::tools::ToolResult],
    merged_blocking: &[cp_base::state::watchers::carriers::WatcherResult],
) {
    // === DEFERRED TEMPO BREAK ===
    // Blocking watchers deferred their tempo decision from pipeline.rs.
    // Now that we have the real results, break tempo if any watcher says so.
    for result in merged_blocking {
        if !result.preserves_tempo {
            app.state.tempo = false;
            break;
        }
    }

    // All resolved — resume normal pipeline: create result message + continue streaming
    let result_id = format!("R{}", app.state.next_result_id);
    let result_global_uid = format!("UID_{}_R", app.state.global_next_uid);
    app.state.next_result_id = app.state.next_result_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);
    let tool_result_records: Vec<ToolResultRecord> = tool_results
        .iter()
        .map(|r| {
            ToolResultRecord::new(r.tool_use_id.clone(), r.content.clone(), r.is_error)
                .display(r.display.clone())
                .tldr(r.tldr.clone())
                .tool_name(r.tool_name.clone())
        })
        .collect();
    let result_msg = Message::new_tool_result(result_id, Some(result_global_uid), tool_result_records);
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
    let new_assistant_msg = Message::new_assistant(assistant_id, assistant_global_uid);
    app.state.messages.push(new_assistant_msg);

    app.state.streaming_estimated_tokens = 0;

    // Accumulate token stats + costs from intermediate stream (same logic as
    // the non-blocking path in pipeline.rs — includes $ computation).
    super::pipeline::accumulate_pending_token_stats(app);

    // Append per-tick cost row (consumes tick_telemetry populated at stream start)
    super::cost_log::append_cost_tsv(&mut app.state);

    app.save_state_async();
    app.state.flags.ui.dirty = true;

    let _r = crate::app::run::streaming::trigger_dirty_panel_refresh(&app.state, &app.cache_tx);
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
            registry.watchers.retain(|w| w.tool_use_id().is_none_or(|tid| !stale_ids.contains(&tid.to_owned())));
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
            ToolResultRecord::new(r.tool_use_id.clone(), interrupted_msg.to_owned(), true)
                .tool_name(r.tool_name.clone())
        })
        .collect();

    let result_msg = Message::new_tool_result(result_id, Some(result_global_uid), tool_result_records);
    app.save_message_async(&result_msg);
    app.state.messages.push(result_msg);
}
