use std::sync::mpsc::{Receiver, Sender};

use crate::app::actions::{Action, ActionResult, apply_action};
use crate::infra::api::{StreamEvent, StreamParams, start_streaming};
use crate::infra::constants::{DEFAULT_WORKER_ID, MAX_API_RETRIES};

use crate::app::App;
use crate::app::context::{get_active_agent_content, prepare_stream_context};
use crate::state::cache::{CacheUpdate, process_cache_request};
use crate::state::{State, get_context_type_meta};

impl App {
    pub(super) fn process_stream_events(&mut self, rx: &Receiver<StreamEvent>) {
        let _guard = crate::profile!("app::stream_events");
        while let Ok(evt) = rx.try_recv() {
            if !self.state.is_streaming {
                continue;
            }
            self.state.dirty = true;
            match evt {
                StreamEvent::Chunk(text) => {
                    self.typewriter.add_chunk(&text);
                }
                StreamEvent::ToolUse(tool) => {
                    self.pending_tools.push(tool);
                }
                StreamEvent::Done { input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason } => {
                    self.typewriter.mark_done();
                    self.pending_done =
                        Some((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason));
                    // API call succeeded — reset retry counter immediately at tick level
                    self.state.api_retry_count = 0;
                }
                StreamEvent::Error(e) => {
                    self.typewriter.reset();
                    // Log every error to disk for debugging
                    let attempt = self.state.api_retry_count + 1;
                    let will_retry = attempt <= MAX_API_RETRIES;
                    let provider = format!("{:?}", self.state.llm_provider);
                    let model = self.state.current_model();
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
                    crate::state::persistence::log_error(&log_msg);

                    // Check if we should retry
                    if will_retry {
                        self.state.api_retry_count += 1;
                        self.pending_retry_error = Some(e);
                    } else {
                        // Max retries reached, show error
                        self.state.api_retry_count = 0;
                        // Track consecutive failed continuations for backoff
                        let spine = cp_mod_spine::SpineState::get_mut(&mut self.state);
                        spine.config.consecutive_continuation_errors += 1;
                        spine.config.last_continuation_error_ms = Some(crate::app::panels::now_ms());
                        apply_action(&mut self.state, Action::StreamError(e));
                    }
                }
            }
        }
    }

    pub(super) fn handle_retry(&mut self, tx: &Sender<StreamEvent>) {
        if let Some(_error) = self.pending_retry_error.take() {
            // Still streaming, retry the request
            if self.state.is_streaming {
                // Clear any partial assistant message content before retrying
                if let Some(msg) = self.state.messages.last_mut()
                    && msg.role == "assistant"
                {
                    msg.content.clear();
                }
                let ctx = prepare_stream_context(&mut self.state, true, None);
                let system_prompt = get_active_agent_content(&self.state);
                self.typewriter.reset();
                self.pending_done = None;
                start_streaming(
                    StreamParams {
                        provider: self.state.llm_provider,
                        model: self.state.current_model(),
                        max_output_tokens: self.state.current_max_output_tokens(),
                        messages: ctx.messages,
                        context_items: ctx.context_items,
                        tools: ctx.tools,
                        system_prompt: system_prompt.clone(),
                        seed_content: Some(system_prompt),
                        worker_id: DEFAULT_WORKER_ID.to_string(),
                    },
                    tx.clone(),
                );
                self.state.dirty = true;
            }
        }
    }

    pub(super) fn process_typewriter(&mut self) {
        let _guard = crate::profile!("app::typewriter");
        if self.state.is_streaming
            && let Some(chars) = self.typewriter.take_chars()
        {
            apply_action(&mut self.state, Action::AppendChars(chars));
            self.state.dirty = true;
        }
    }

    pub(super) fn process_api_check_results(&mut self) {
        if let Some(rx) = &self.api_check_rx
            && let Ok(result) = rx.try_recv()
        {
            self.state.api_check_in_progress = false;
            self.state.api_check_result = Some(result);
            self.state.dirty = true;
            self.api_check_rx = None;
            self.save_state_async();
        }
    }

    /// Continue streaming after tool execution (called when panels are ready).
    pub(super) fn continue_streaming(&mut self, tx: &Sender<StreamEvent>) {
        self.state.is_tooling = false;
        let ctx = prepare_stream_context(&mut self.state, true, None);
        let system_prompt = get_active_agent_content(&self.state);
        self.typewriter.reset();
        self.pending_done = None;
        start_streaming(
            StreamParams {
                provider: self.state.llm_provider,
                model: self.state.current_model(),
                max_output_tokens: self.state.current_max_output_tokens(),
                messages: ctx.messages,
                context_items: ctx.context_items,
                tools: ctx.tools,
                system_prompt: system_prompt.clone(),
                seed_content: Some(system_prompt),
                worker_id: DEFAULT_WORKER_ID.to_string(),
            },
            tx.clone(),
        );
    }

    pub(super) fn finalize_stream(&mut self) {
        if !self.state.is_streaming {
            return;
        }
        // Don't finalize while waiting for panels or deferred sleep —
        // pending_done is still Some from the intermediate stream, and
        // continue_streaming will clear it when the deferred state resolves.
        if self.state.waiting_for_panels || self.deferred_tool_sleeping {
            return;
        }
        // Don't finalize while a question form is pending user response
        if self.pending_question_tool_results.is_some() {
            return;
        }
        // Don't finalize while a console blocking wait is pending
        if self.pending_console_wait_tool_results.is_some() {
            return;
        }

        if let Some((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, ref stop_reason)) =
            self.pending_done
            && self.typewriter.pending_chars.is_empty()
            && self.pending_tools.is_empty()
        {
            self.state.dirty = true;
            let stop_reason = stop_reason.clone();
            match apply_action(
                &mut self.state,
                Action::StreamDone {
                    _input_tokens: input_tokens,
                    output_tokens,
                    cache_hit_tokens,
                    cache_miss_tokens,
                    stop_reason,
                },
            ) {
                ActionResult::SaveMessage(id) => {
                    if let Some(msg) = self.state.messages.iter().find(|m| m.id == id) {
                        self.save_message_async(msg);
                    }
                    self.save_state_async();
                }
                ActionResult::Save => self.save_state_async(),
                _ => {}
            }
            // Reset auto-continuation count on each successful tick (stream completion).
            // This means MaxAutoRetries only fires on consecutive *failed* continuations,
            // not on total auto-continuations in an autonomous session.
            {
                let spine_cfg = &mut cp_mod_spine::SpineState::get_mut(&mut self.state).config;
                spine_cfg.auto_continuation_count = 0;
                // Reset consecutive error backoff — successful completion proves API is healthy
                spine_cfg.consecutive_continuation_errors = 0;
                spine_cfg.last_continuation_error_ms = None;
                // Reopen the throttle gate — a successful LLM tick means the system processed
                // the notifications. New notifications can now trigger auto-continuation again.
                spine_cfg.can_awake_using_notification = true;
            }

            // Unblock any guard-rail-blocked notifications — they get another chance now
            // that a stream has completed successfully.
            cp_mod_spine::SpineState::unblock_all(&mut self.state);

            self.typewriter.reset();
            self.pending_done = None;
        }
    }
}

// ─── Panel Wait Helpers ─────────────────────────────────────────────────────

/// Check if any async-wait panels have cache_deprecated = true.
pub(super) fn has_dirty_panels(state: &State) -> bool {
    state.context.iter().any(|c| {
        get_context_type_meta(c.context_type.as_str()).map(|m| m.needs_async_wait).unwrap_or(false)
            && c.cache_deprecated
    })
}

/// Check if any async-wait panels need refresh before continuing the stream.
pub(super) fn has_dirty_file_panels(state: &State) -> bool {
    state.context.iter().any(|c| {
        get_context_type_meta(c.context_type.as_str()).map(|m| m.needs_async_wait).unwrap_or(false)
            && c.cache_deprecated
    })
}

/// Trigger immediate cache refresh for all dirty async-wait panels.
/// Returns true if any panels needed refresh.
pub(super) fn trigger_dirty_panel_refresh(state: &State, cache_tx: &Sender<CacheUpdate>) -> bool {
    let mut any_triggered = false;
    for ctx in &state.context {
        let needs_wait = get_context_type_meta(ctx.context_type.as_str()).map(|m| m.needs_async_wait).unwrap_or(false);
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
