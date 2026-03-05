use std::sync::mpsc::Sender;

use crate::app::actions::clean_llm_id_prefix;
use crate::app::panels::now_ms;
use crate::infra::api::StreamEvent;
use crate::infra::tools::{execute_tool, perform_reload};
use crate::modules::pre_flight::pre_flight_tool;
use crate::state::persistence::build_message_op;
use crate::state::{Message, MessageStatus, MessageType, ToolResultRecord, ToolUseRecord};

use super::streaming::{has_dirty_file_panels, has_dirty_panels, trigger_dirty_panel_refresh};
use cp_mod_callback::firing as callback_firing;
use cp_mod_callback::trigger as callback_trigger;
use cp_mod_console::CONSOLE_WAIT_BLOCKING_SENTINEL;
use cp_mod_queue::QueueState;

use crate::app::App;

// ─── Tool pipeline ──────────────────────────────────────────────────────────

impl App {
    pub(super) fn handle_tool_execution(&mut self, tx: &Sender<StreamEvent>) {
        if !self.state.flags.stream.is_streaming
            || self.pending_done.is_none()
            || !self.typewriter.pending_chars.is_empty()
            || self.pending_tools.is_empty()
        {
            return;
        }
        // Don't process new tools while waiting for panels or deferred sleep
        if self.state.flags.lifecycle.waiting_for_panels || self.deferred_tool_sleeping {
            return;
        }
        // Don't process tools while a question form is pending user response
        if self.state.get_ext::<cp_base::ui::PendingQuestionForm>().is_some() {
            return;
        }
        let _guard = crate::profile!("app::tool_exec");

        self.state.flags.ui.dirty = true;
        self.state.flags.stream.is_tooling = true;
        let tools = std::mem::take(&mut self.pending_tools);
        let mut tool_results: Vec<crate::infra::tools::ToolResult> = Vec::new();

        // Finalize current assistant message
        if let Some(msg) = self.state.messages.last_mut()
            && msg.role == "assistant"
        {
            // Clean any LLM ID prefixes before saving
            msg.content = clean_llm_id_prefix(&msg.content);
            let op = build_message_op(msg);
            self.writer.send_message(op);
        }

        // Create tool call messages
        for tool in &tools {
            let tool_id = format!("T{}", self.state.next_tool_id);
            let tool_uid = format!("UID_{}_T", self.state.global_next_uid);
            self.state.next_tool_id += 1;
            self.state.global_next_uid += 1;

            let tool_msg = Message {
                id: tool_id,
                uid: Some(tool_uid),
                role: "assistant".to_string(),
                message_type: MessageType::ToolCall,
                content: String::new(),
                content_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: vec![ToolUseRecord {
                    id: tool.id.clone(),
                    name: tool.name.clone(),
                    input: tool.input.clone(),
                }],
                tool_results: Vec::new(),
                input_tokens: 0,
                timestamp_ms: now_ms(),
            };
            self.save_message_async(&tool_msg);
            self.state.messages.push(tool_msg);

            let result = if tool.name == "Queue_execute" {
                // Queue flush: execute all queued calls through normal dispatch
                super::tool_cleanup::execute_queue_flush(tool, &mut self.state)
            } else {
                // Pre-flight: schema check + module semantic check (ALWAYS runs, queue or not)
                let pf = pre_flight_tool(tool, &self.state, &self.state.active_modules.clone());
                if pf.has_errors() {
                    // Hard stop — don't queue, don't execute
                    crate::infra::tools::ToolResult::new(tool.id.clone(), pf.format_errors(), true)
                } else if QueueState::get(&self.state).active && !QueueState::is_queue_tool(&tool.name) {
                    // Queue intercept: enqueue instead of executing
                    let qs = QueueState::get_mut(&mut self.state);
                    let idx = qs.enqueue(tool.name.clone(), tool.id.clone(), tool.input.clone(), now_ms());
                    let params = serde_json::to_string(&tool.input).unwrap_or_default();
                    let short = if params.len() > 120 {
                        let mut end = 117;
                        while !params.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &params[..end])
                    } else {
                        params
                    };
                    let mut msg = format!("Queued as #{}: {}({})", idx, tool.name, short);
                    if pf.has_warnings() {
                        msg.push_str(&format!("\n{}", pf.format_errors()));
                    }
                    crate::infra::tools::ToolResult::new(tool.id.clone(), msg, false)
                } else {
                    // Execute normally
                    let mut result = execute_tool(tool, &mut self.state);
                    if pf.has_warnings() {
                        result.content.push_str(&format!("\n{}", pf.format_errors()));
                    }
                    result
                }
            };
            tool_results.push(result);
        }

        // Check if any tool triggered a question form (blocking)
        let has_pending_question = tool_results.iter().any(|r| r.content == "__QUESTION_PENDING__");
        if has_pending_question {
            // Don't create result message or continue streaming yet.
            // The form is active — when user submits/dismisses, check_question_form()
            // will replace the placeholder and resume the pipeline.
            // Store the pending tool results for later resolution.
            self.pending_question_tool_results = Some(tool_results);
            self.save_state_async();
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
                let _ = crate::app::reverie::trigger::start_manual_reverie(&mut self.state, agent_id, context);
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
            let (matched, skip_warnings) = callback_trigger::match_callbacks(&self.state, &changed_files);

            // Inject skip_callbacks warnings into tool results so the AI sees them
            if !skip_warnings.is_empty() {
                let warning_note = format!("\n\n[skip_callbacks warnings: {}]", skip_warnings.join("; "));
                for tr in tool_results.iter_mut().rev() {
                    if tr.tool_name == "Edit" || tr.tool_name == "Write" {
                        tr.content.push_str(&warning_note);
                        break;
                    }
                }
            }

            if !matched.is_empty() {
                let (blocking_cbs, async_cbs) = callback_trigger::partition_callbacks(matched);

                // Fire non-blocking callbacks immediately (they run async via watchers)
                if !async_cbs.is_empty() {
                    let summaries = callback_firing::fire_async_callbacks(&mut self.state, &async_cbs);
                    // Append compact callback summary to the last Edit/Write tool result
                    if !summaries.is_empty() {
                        let note = format!("\nCallbacks:\n{}", summaries.join("\n"));
                        // Find the last Edit/Write tool result and append the note
                        for tr in tool_results.iter_mut().rev() {
                            if tr.tool_name == "Edit" || tr.tool_name == "Write" {
                                tr.content.push_str(&note);
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
                    let sentinel_id = format!("cb_block_{}", self.state.next_tool_id);
                    self.state.next_tool_id += 1;

                    let _summaries =
                        callback_firing::fire_blocking_callbacks(&mut self.state, &blocking_cbs, &sentinel_id);

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
            self.pending_console_wait_tool_results = Some(tool_results);
            self.save_state_async();
            return;
        }

        // Create tool result message
        let result_id = format!("R{}", self.state.next_result_id);
        let result_uid = format!("UID_{}_R", self.state.global_next_uid);
        self.state.next_result_id += 1;
        self.state.global_next_uid += 1;
        let tool_result_records: Vec<ToolResultRecord> = tool_results
            .iter()
            .zip(tools.iter())
            .map(|(r, t)| ToolResultRecord {
                tool_use_id: r.tool_use_id.clone(),
                content: r.content.clone(),
                is_error: r.is_error,
                tool_name: t.name.clone(),
            })
            .collect();
        let result_msg = Message {
            id: result_id,
            uid: Some(result_uid),
            role: "user".to_string(),
            message_type: MessageType::ToolResult,
            content: String::new(),
            content_token_count: 0,
            status: MessageStatus::Full,
            tool_uses: Vec::new(),
            tool_results: tool_result_records,
            input_tokens: 0,
            timestamp_ms: now_ms(),
        };
        self.save_message_async(&result_msg);
        self.state.messages.push(result_msg);

        // Check if reload was requested - exit pipeline, main loop will break
        if self.state.flags.lifecycle.reload_pending {
            perform_reload(&self.state);
            return;
        }

        // Create new assistant message
        let assistant_id = format!("A{}", self.state.next_assistant_id);
        let assistant_uid = format!("UID_{}_A", self.state.global_next_uid);
        self.state.next_assistant_id += 1;
        self.state.global_next_uid += 1;
        let new_assistant_msg = Message {
            id: assistant_id,
            uid: Some(assistant_uid),
            role: "assistant".to_string(),
            message_type: MessageType::TextMessage,
            content: String::new(),
            content_token_count: 0,
            status: MessageStatus::Full,
            tool_uses: Vec::new(),
            tool_results: Vec::new(),
            input_tokens: 0,
            timestamp_ms: now_ms(),
        };
        self.state.messages.push(new_assistant_msg);

        self.state.streaming_estimated_tokens = 0;

        // Accumulate token stats from intermediate stream before discarding pending_done
        if let Some((_, output_tokens, cache_hit_tokens, cache_miss_tokens, _)) = self.pending_done {
            self.state.tick_cache_hit_tokens = cache_hit_tokens;
            self.state.tick_cache_miss_tokens = cache_miss_tokens;
            self.state.tick_output_tokens = output_tokens;
            self.state.stream_cache_hit_tokens += cache_hit_tokens;
            self.state.stream_cache_miss_tokens += cache_miss_tokens;
            self.state.stream_output_tokens += output_tokens;
            self.state.cache_hit_tokens += cache_hit_tokens;
            self.state.cache_miss_tokens += cache_miss_tokens;
            self.state.total_output_tokens += output_tokens;
        }

        self.save_state_async();

        // Check if any tool requested a sleep (e.g., console send_keys delay)
        if self.state.tool_sleep_until_ms > 0 {
            // Defer everything — main loop will check timer and continue
            self.deferred_tool_sleeping = true;
            self.deferred_tool_sleep_until_ms = self.state.tool_sleep_until_ms;
            self.state.tool_sleep_until_ms = 0; // Clear from state (App owns it now)
            return;
        }

        // Trigger background cache refresh for dirty file panels (non-blocking)
        let _ = trigger_dirty_panel_refresh(&self.state, &self.cache_tx);

        // Check if we need to wait for panels before continuing stream
        if has_dirty_file_panels(&self.state) {
            // Set waiting flag — main loop will check and continue streaming when ready
            self.state.flags.lifecycle.waiting_for_panels = true;
            self.wait_started_ms = now_ms();
        } else {
            // No dirty panels — continue streaming immediately
            self.continue_streaming(tx);
        }
    }

    /// Non-blocking check: if we're waiting for file panels to load,
    /// check if they're ready (or timed out) and continue streaming.
    pub(super) fn check_waiting_for_panels(&mut self, tx: &Sender<StreamEvent>) {
        if !self.state.flags.lifecycle.waiting_for_panels {
            return;
        }

        let panels_ready = !has_dirty_panels(&self.state);
        let timed_out = now_ms().saturating_sub(self.wait_started_ms) >= 5_000;

        if panels_ready || timed_out {
            self.state.flags.lifecycle.waiting_for_panels = false;
            self.state.flags.ui.dirty = true;
            self.continue_streaming(tx);
        }
    }

    /// Non-blocking check: if a tool requested a sleep (e.g., `console_sleep`),
    /// wait for the timer to expire, then deprecate tmux panels and continue
    /// through the normal `wait_for_panels` → `continue_streaming` pipeline.
    pub(super) fn check_deferred_sleep(&mut self, tx: &Sender<StreamEvent>) {
        if !self.deferred_tool_sleeping {
            return;
        }

        if now_ms() < self.deferred_tool_sleep_until_ms {
            return; // Still sleeping — keep processing input normally
        }

        self.deferred_tool_sleeping = false;
        self.deferred_tool_sleep_until_ms = 0;
        self.state.flags.ui.dirty = true;

        // Deferred sleep expired — continue streaming
        self.continue_streaming(tx);
    }

    /// Non-blocking check: if the user has resolved a pending question form,
    /// replace the `__QUESTION_PENDING__` placeholder with the real answer and
    /// resume the tool pipeline (create result message + continue streaming).
    pub(super) fn check_question_form(&mut self, tx: &Sender<StreamEvent>) {
        // Only check if we have pending tool results waiting on a question
        if self.pending_question_tool_results.is_none() {
            return;
        }

        // Check if form is resolved
        let resolved = self.state.get_ext::<cp_base::ui::PendingQuestionForm>().is_some_and(|f| f.resolved);

        if !resolved {
            return;
        }

        // Extract the resolved form and remove it from state
        let Some(form) = self
            .state
            .module_data
            .remove(&std::any::TypeId::of::<cp_base::ui::PendingQuestionForm>())
            .and_then(|v| v.downcast::<cp_base::ui::PendingQuestionForm>().ok())
        else {
            return;
        };

        let result_json =
            form.result_json.unwrap_or_else(|| r#"{"dismissed":true,"message":"User declined to answer"}"#.to_string());

        // Replace placeholder in pending tool results
        let Some(mut tool_results) = self.pending_question_tool_results.take() else {
            return;
        };
        for tr in &mut tool_results {
            if tr.content == "__QUESTION_PENDING__" {
                tr.content.clone_from(&result_json);
            }
        }

        // Now resume the normal pipeline: create result message and continue streaming
        let result_id = format!("R{}", self.state.next_result_id);
        let result_uid = format!("UID_{}_R", self.state.global_next_uid);
        self.state.next_result_id += 1;
        self.state.global_next_uid += 1;
        let tool_result_records: Vec<ToolResultRecord> = tool_results
            .iter()
            .map(|r| ToolResultRecord {
                tool_use_id: r.tool_use_id.clone(),
                content: r.content.clone(),
                is_error: r.is_error,
                tool_name: r.tool_name.clone(),
            })
            .collect();
        let result_msg = Message {
            id: result_id,
            uid: Some(result_uid),
            role: "user".to_string(),
            message_type: MessageType::ToolResult,
            content: String::new(),
            content_token_count: 0,
            status: MessageStatus::Full,
            tool_uses: Vec::new(),
            tool_results: tool_result_records,
            input_tokens: 0,
            timestamp_ms: now_ms(),
        };
        self.save_message_async(&result_msg);
        self.state.messages.push(result_msg);

        // Check if reload was requested
        if self.state.flags.lifecycle.reload_pending {
            perform_reload(&self.state);
            return;
        }

        // Create new assistant message for continued streaming
        let assistant_id = format!("A{}", self.state.next_assistant_id);
        let assistant_uid = format!("UID_{}_A", self.state.global_next_uid);
        self.state.next_assistant_id += 1;
        self.state.global_next_uid += 1;
        let new_assistant_msg = Message {
            id: assistant_id,
            uid: Some(assistant_uid),
            role: "assistant".to_string(),
            message_type: MessageType::TextMessage,
            content: String::new(),
            content_token_count: 0,
            status: MessageStatus::Full,
            tool_uses: Vec::new(),
            tool_results: Vec::new(),
            input_tokens: 0,
            timestamp_ms: now_ms(),
        };
        self.state.messages.push(new_assistant_msg);

        self.state.streaming_estimated_tokens = 0;

        // Accumulate token stats from intermediate stream
        if let Some((_, output_tokens, cache_hit_tokens, cache_miss_tokens, _)) = self.pending_done {
            self.state.tick_cache_hit_tokens = cache_hit_tokens;
            self.state.tick_cache_miss_tokens = cache_miss_tokens;
            self.state.tick_output_tokens = output_tokens;
            self.state.stream_cache_hit_tokens += cache_hit_tokens;
            self.state.stream_cache_miss_tokens += cache_miss_tokens;
            self.state.stream_output_tokens += output_tokens;
            self.state.cache_hit_tokens += cache_hit_tokens;
            self.state.cache_miss_tokens += cache_miss_tokens;
            self.state.total_output_tokens += output_tokens;
        }

        self.save_state_async();
        self.state.flags.ui.dirty = true;

        // Continue streaming
        let _ = trigger_dirty_panel_refresh(&self.state, &self.cache_tx);
        if has_dirty_file_panels(&self.state) {
            self.state.flags.lifecycle.waiting_for_panels = true;
            self.wait_started_ms = now_ms();
        } else {
            self.continue_streaming(tx);
        }
    }
}
