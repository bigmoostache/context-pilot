//! Reverie event processing — polls reverie streams and dispatches tools.
//! Supports multiple concurrent reveries (one per agent type).

use std::sync::mpsc;

use crate::app::App;
use crate::app::reverie::{streaming, tools};
use crate::infra::api::StreamEvent;
use crate::state::persistence::save_state;
use cp_base::config::REVERIE;
use cp_mod_queue::QueueState;

impl App {
    /// Check if any reverie needs a stream started (state has reverie but no stream).
    /// Called from the main event loop.
    pub(super) fn maybe_start_reverie_stream(&mut self) {
        // Collect agent_ids that need a stream started
        let needs_start: Vec<String> = self
            .state
            .reveries
            .iter()
            .filter(|(agent_id, r)| r.is_streaming && !self.reverie_streams.contains_key(*agent_id))
            .map(|(agent_id, _)| agent_id.clone())
            .collect();

        for agent_id in needs_start {
            let (tx, rx) = mpsc::channel();
            streaming::start_reverie_stream(&mut self.state, &agent_id, tx);
            let _r = self
                .reverie_streams
                .insert(agent_id, super::super::ReverieStream { rx, pending_tools: Vec::new(), report_called: false });
        }
    }

    /// Poll all reverie streams for events and process them.
    /// Called from the main event loop, AFTER main stream events.
    pub(super) fn process_reverie_events(&mut self) {
        // Collect agent_ids that have streams
        let agent_ids: Vec<String> = self.reverie_streams.keys().cloned().collect();

        for agent_id in agent_ids {
            // Drain all events from this reverie's stream
            let events: Vec<StreamEvent> = match self.reverie_streams.get(&agent_id) {
                Some(s) => s.rx.try_iter().collect(),
                None => continue,
            };

            for evt in events {
                self.state.flags.ui.dirty = true;
                match evt {
                    StreamEvent::ToolProgress { .. } => {} // Reveries run in background — no UI preview
                    StreamEvent::Chunk(text) => {
                        if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                            if rev.messages.last().is_none_or(|m| m.role != "assistant") {
                                rev.messages.push(crate::state::Message {
                                    id: format!("rev-{}", rev.messages.len()),
                                    uid: None,
                                    role: "assistant".to_string(),
                                    content: String::new(),
                                    message_type: crate::state::MessageType::TextMessage,
                                    status: crate::state::MessageStatus::Full,
                                    content_token_count: 0,
                                    input_tokens: 0,
                                    timestamp_ms: crate::app::panels::now_ms(),
                                    tool_uses: Vec::new(),
                                    tool_results: Vec::new(),
                                });
                            }
                            if let Some(msg) = rev.messages.last_mut() {
                                msg.content.push_str(&text);
                            }
                        }
                    }
                    StreamEvent::ToolUse(tool) => {
                        if let Some(stream) = self.reverie_streams.get_mut(&agent_id) {
                            stream.pending_tools.push(tool);
                        }
                    }
                    StreamEvent::Done {
                        input_tokens: _,
                        output_tokens: _,
                        cache_hit_tokens: _,
                        cache_miss_tokens: _,
                        stop_reason: _,
                    } => {
                        if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                            if let Some(msg) = rev.messages.last_mut() {
                                msg.status = crate::state::MessageStatus::Full;
                            }
                            rev.is_streaming = false;
                        }
                    }
                    StreamEvent::Error(e) => {
                        // Reverie errors are non-critical — log and destroy this agent's session
                        let _r = cp_mod_spine::SpineState::create_notification(
                            &mut self.state,
                            cp_mod_spine::NotificationType::Custom,
                            "Reverie".to_string(),
                            format!("Reverie '{agent_id}' error: {e}. Destroying session."),
                        );
                        // Discard any queued actions from the failed reverie
                        QueueState::get_mut(&mut self.state).clear();
                        let _r = self.state.reveries.remove(&agent_id);
                        let _r = self.reverie_streams.remove(&agent_id);
                        break; // This agent's stream is gone, move to next
                    }
                }
            }
        }
    }

    /// Execute pending reverie tool calls for all active reveries.
    /// Called from the main event loop, AFTER main tools are processed.
    pub(super) fn handle_reverie_tools(&mut self) {
        // Collect agent_ids that have pending tools
        let agent_ids: Vec<String> = self
            .reverie_streams
            .iter()
            .filter(|(_, s)| !s.pending_tools.is_empty())
            .map(|(id, _)| id.clone())
            .collect();

        for agent_id in agent_ids {
            // Take pending tools from the stream state
            let pending = match self.reverie_streams.get_mut(&agent_id) {
                Some(s) => std::mem::take(&mut s.pending_tools),
                None => continue,
            };

            let mut tool_results = Vec::new();

            for tool in &pending {
                // Increment tool call count
                if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                    rev.tool_call_count += 1;
                }

                // Check tool cap guard rail
                let cap = crate::infra::constants::REVERIE_TOOL_CAP;
                if self.state.reveries.get(&agent_id).is_some_and(|r| r.tool_call_count > cap) {
                    let _r = cp_mod_spine::SpineState::create_notification(
                        &mut self.state,
                        cp_mod_spine::NotificationType::Custom,
                        "Reverie".to_string(),
                        format!("Tool cap ({cap}) reached for '{agent_id}'. Force-stopping."),
                    );
                    QueueState::get_mut(&mut self.state).clear();
                    let _r = self.state.reveries.remove(&agent_id);
                    let _r = self.reverie_streams.remove(&agent_id);
                    break; // Move to next agent
                }

                // Dispatch through reverie tool router
                // Queue_execute needs special handling (flush lives in tool_cleanup, not the module)
                let result = if tool.name == "Queue_execute" {
                    // Reverie doesn't need flushed tools (no callbacks) — just the summary
                    super::tool_cleanup::execute_queue_flush(tool, &mut self.state).0
                } else if tool.name == "Queue_activate" {
                    // Toggle the reverie's own queue flag, NOT the main worker's QueueState.active
                    if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                        rev.queue_active = true;
                    }
                    crate::infra::tools::ToolResult::new(tool.id.clone(), "Queue activated (reverie)".into(), false)
                } else if tool.name == "Queue_pause" {
                    if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                        rev.queue_active = false;
                    }
                    crate::infra::tools::ToolResult::new(tool.id.clone(), "Queue paused (reverie)".into(), false)
                } else if tool.name == "Queue_empty" {
                    if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                        rev.queue_active = false;
                    }
                    QueueState::get_mut(&mut self.state).clear();
                    crate::infra::tools::ToolResult::new(tool.id.clone(), "Queue emptied (reverie)".into(), false)
                } else if let Some(result) = tools::dispatch_reverie_tool(tool, &self.state) {
                    // Check for Report sentinel
                    if result.content.starts_with("REVERIE_REPORT:") {
                        let summary = result.content.strip_prefix("REVERIE_REPORT:").unwrap_or("Completed");
                        let _r = cp_mod_spine::SpineState::create_notification(
                            &mut self.state,
                            cp_mod_spine::NotificationType::Custom,
                            "Reverie".to_string(),
                            summary.to_string(),
                        );
                        if let Some(stream) = self.reverie_streams.get_mut(&agent_id) {
                            stream.report_called = true;
                        }
                        // Clear queued actions from this reverie (shared queue) but
                        // do NOT touch QueueState.active — that's the main worker's toggle.
                        QueueState::get_mut(&mut self.state).clear();
                        // Destroy this agent's reverie
                        let _r = self.state.reveries.remove(&agent_id);
                        let _r = self.reverie_streams.remove(&agent_id);
                        save_state(&self.state);
                        break; // Move to next agent
                    }
                    result
                } else {
                    // Tool is allowed — check if reverie queue is active
                    let should_queue = self.state.reveries.get(&agent_id).is_some_and(|r| r.queue_active)
                        && !QueueState::is_queue_tool(&tool.name);
                    if should_queue {
                        let qs = QueueState::get_mut(&mut self.state);
                        let idx = qs.enqueue(
                            tool.name.clone(),
                            tool.id.clone(),
                            tool.input.clone(),
                            crate::app::panels::now_ms(),
                        );
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
                        crate::infra::tools::ToolResult::new(
                            tool.id.clone(),
                            format!("Queued as #{}: {}({})", idx, tool.name, short),
                            false,
                        )
                    } else {
                        // Execute normally through module dispatch
                        let active = self.state.active_modules.clone();
                        crate::modules::dispatch_tool(tool, &mut self.state, &active)
                    }
                };

                // Record tool use + result in reverie messages
                if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                    rev.messages.push(crate::state::Message {
                        id: format!("rev-tc-{}", rev.messages.len()),
                        uid: None,
                        role: "assistant".to_string(),
                        content: String::new(),
                        message_type: crate::state::MessageType::ToolCall,
                        status: crate::state::MessageStatus::Full,
                        content_token_count: 0,
                        input_tokens: 0,
                        timestamp_ms: crate::app::panels::now_ms(),
                        tool_uses: vec![crate::state::ToolUseRecord {
                            id: tool.id.clone(),
                            name: tool.name.clone(),
                            input: tool.input.clone(),
                        }],
                        tool_results: Vec::new(),
                    });
                    rev.messages.push(crate::state::Message {
                        id: format!("rev-tr-{}", rev.messages.len()),
                        uid: None,
                        role: "user".to_string(),
                        content: String::new(),
                        message_type: crate::state::MessageType::ToolResult,
                        status: crate::state::MessageStatus::Full,
                        content_token_count: 0,
                        input_tokens: 0,
                        timestamp_ms: crate::app::panels::now_ms(),
                        tool_uses: Vec::new(),
                        tool_results: vec![crate::state::ToolResultRecord {
                            tool_use_id: result.tool_use_id.clone(),
                            tool_name: result.tool_name.clone(),
                            content: result.content.clone(),
                            is_error: result.is_error,
                        }],
                    });
                }
                tool_results.push(result);
            }

            // If we have tool results and this reverie is still alive, re-stream
            if !tool_results.is_empty() && self.state.reveries.contains_key(&agent_id) {
                // Trim trailing whitespace from assistant messages
                if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                    for msg in &mut rev.messages {
                        if msg.role == "assistant" {
                            msg.content = msg.content.trim_end().to_string();
                        }
                    }
                    rev.is_streaming = true;
                }
                let (tx, rx) = mpsc::channel();
                streaming::start_reverie_stream(&mut self.state, &agent_id, tx);
                let _r = self.reverie_streams.insert(
                    agent_id,
                    super::super::ReverieStream { rx, pending_tools: Vec::new(), report_called: false },
                );
            }
        }
    }

    /// Check if any reverie ended without calling Report.
    /// If so, inject a user message telling it to call Report, then re-stream.
    pub(super) fn check_reverie_end_turn(&mut self) {
        // Collect agent_ids of reveries that have stopped streaming
        let stopped: Vec<String> =
            self.state.reveries.iter().filter(|(_, r)| !r.is_streaming).map(|(id, _)| id.clone()).collect();

        for agent_id in stopped {
            let report_called = self.reverie_streams.get(&agent_id).is_some_and(|s| s.report_called);

            if report_called {
                continue; // All good
            }

            // End turn without Report — check retry limit
            let retries = self.state.reveries.get(&agent_id).map_or(0, |r| r.report_retries);
            if retries >= 1 {
                // Max retries reached — force destroy
                let _r = cp_mod_spine::SpineState::create_notification(
                    &mut self.state,
                    cp_mod_spine::NotificationType::Custom,
                    "Reverie".to_string(),
                    format!("Reverie '{agent_id}' ended without Report after retry. Force-destroying."),
                );
                QueueState::get_mut(&mut self.state).clear();
                let _r = self.state.reveries.remove(&agent_id);
                let _r = self.reverie_streams.remove(&agent_id);
                continue;
            }

            // Inject a user message telling the LLM to call Report, then re-stream
            if let Some(rev) = self.state.reveries.get_mut(&agent_id) {
                rev.report_retries += 1;
                rev.is_streaming = true;

                for msg in &mut rev.messages {
                    if msg.role == "assistant" {
                        msg.content = msg.content.trim_end().to_string();
                    }
                }

                rev.messages.push(crate::state::Message {
                    id: format!("rev-nudge-{}", rev.messages.len()),
                    uid: None,
                    role: "user".to_string(),
                    content: REVERIE.report_nudge.trim_end().to_string(),
                    message_type: crate::state::MessageType::TextMessage,
                    status: crate::state::MessageStatus::Full,
                    content_token_count: 0,
                    input_tokens: 0,
                    timestamp_ms: crate::app::panels::now_ms(),
                    tool_uses: Vec::new(),
                    tool_results: Vec::new(),
                });
            }

            let (tx, rx) = mpsc::channel();
            streaming::start_reverie_stream(&mut self.state, &agent_id, tx);
            let _r = self
                .reverie_streams
                .insert(agent_id, super::super::ReverieStream { rx, pending_tools: Vec::new(), report_called: false });
        }
    }
}
