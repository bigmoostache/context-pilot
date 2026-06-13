use cp_base::state::data::model_helpers::ModelPricing as _;
use std::io;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crossterm::event;
use ratatui::prelude::{CrosstermBackend, Terminal};

use crate::app::actions::{Action, ActionResult, apply_action};
use crate::app::events::handle_event;
use crate::app::panels::now_ms;
use crate::infra::api::{StreamEvent, start_streaming};
use crate::infra::constants::{EVENT_POLL_MS, RENDER_THROTTLE_MS};
use crate::state::Kind;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::{check_ownership, save_state};
use crate::ui;

use crate::app::App;
use crate::app::context::{build_stream_params, get_active_agent_content, prepare_stream_context};
use cp_base::state::data::message::{MsgKind, MsgStatus, ToolResultRecord, ToolUseRecord};
use cp_base::tools::ToolUse;
use cp_mod_spine::engine::{SpineDecision, apply_continuation, check_spine};
use cp_mod_spine::types::{NotificationType, SpineState};
use cp_mod_threads::types::{FocusState, ThreadStatus, ThreadsState};

/// Bundles the I/O channels polled by the main event loop.
pub(crate) struct EventChannels<'ch> {
    /// Sends stream events to the LLM provider thread.
    pub tx: &'ch Sender<StreamEvent>,
    /// Receives stream events from the LLM provider thread.
    pub rx: &'ch Receiver<StreamEvent>,
    /// Receives cache update results from the background hasher.
    pub cache_rx: &'ch Receiver<CacheUpdate>,
}

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Main event loop: processes input, stream events, tools, spine, and rendering.
    pub(crate) fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        ch: &EventChannels<'_>,
    ) -> io::Result<()> {
        // Initial cache setup - watch files and schedule initial refreshes
        super::watchers::setup_file_watchers(self);
        super::watchers::sync_gh_watches(self);
        super::watchers::schedule_initial_cache_refreshes(self);

        // Claim ownership immediately
        save_state(&self.state);

        // Auto-resume streaming if flag was set (e.g., after reload_tui)
        if self.resume_stream {
            self.resume_stream = false;
            let _r = SpineState::create_notification(
                &mut self.state,
                NotificationType::ReloadResume,
                "reload_resume".to_string(),
                "Resuming after TUI reload".to_string(),
            );
            save_state(&self.state);
        }

        loop {
            let current_ms = now_ms();
            let _fg = cp_base::flame!("loop");

            // === INPUT FIRST: Process user input with minimal latency ===
            // Non-blocking check for input - handle immediately for responsive feel
            if event::poll(Duration::ZERO)? {
                let evt = event::read()?;

                // Handle command palette events first if it's open
                if self.command_palette.is_open {
                    if let Some(action) = self.handle_palette_event(&evt) {
                        self.handle_action(action, ch.tx);
                    }
                    self.state.flags.ui.dirty = true;

                    // Render immediately after input for instant feedback
                    if self.state.flags.ui.dirty {
                        let _r = terminal.draw(|frame| {
                            ui::render(frame, &mut self.state);
                            self.command_palette.render(frame, &self.state);
                        })?;
                        self.state.flags.ui.dirty = false;
                        self.last_render_ms = current_ms;
                    }
                    continue;
                }

                // Handle autocomplete events if popup is active
                if let Some(ac) = self.state.get_ext::<cp_base::state::autocomplete::Suggestions>()
                    && ac.active
                {
                    self.handle_autocomplete_event(&evt);
                    self.state.flags.ui.dirty = true;

                    // Render immediately
                    if self.state.flags.ui.dirty {
                        let _r = terminal.draw(|frame| {
                            ui::render(frame, &mut self.state);
                            self.command_palette.render(frame, &self.state);
                        })?;
                        self.state.flags.ui.dirty = false;
                        self.last_render_ms = current_ms;
                    }
                    continue;
                }

                let Some(action) = handle_event(&evt, &self.state) else {
                    // User quit — flush all pending writes and save final state synchronously
                    self.writer.flush();
                    save_state(&self.state);
                    break;
                };

                // Check for Ctrl+P to open palette
                if matches!(action, Action::OpenCommandPalette) {
                    self.command_palette.open(&self.state);
                    self.state.flags.ui.dirty = true;
                } else {
                    self.handle_action(action, ch.tx);
                }

                // Render immediately after input for instant feedback
                if self.state.flags.ui.dirty {
                    let _r = terminal.draw(|frame| {
                        ui::render(frame, &mut self.state);
                        self.command_palette.render(frame, &self.state);
                    })?;
                    self.state.flags.ui.dirty = false;
                    self.last_render_ms = current_ms;
                }
            }

            // === BACKGROUND PROCESSING ===
            super::streaming::process_stream_events(self, ch.rx);
            super::streaming::handle_retry(self, ch.tx);
            super::streaming::process_typewriter(self);
            super::watchers::process_cache_updates(self, ch.cache_rx);
            super::watchers::process_watcher_events(self);
            // Check if we're waiting for panels and they're ready (non-blocking)
            super::tools::checks::check_waiting_for_panels(self, ch.tx);
            // Check if deferred sleep timer has expired (non-blocking)
            super::tools::checks::check_deferred_sleep(self, ch.tx);
            // Check watchers (blocking sentinel replacement + async → spine notifications)
            super::tools::cleanup::check_watchers(self, ch.tx);
            // Throttle gh watcher sync to every 5 seconds (mutex lock + iteration)
            if current_ms.saturating_sub(self.last_gh_sync_ms) >= 5_000 {
                self.last_gh_sync_ms = current_ms;
                super::watchers::sync_gh_watches(self);
            }
            // Drain Matrix sync events periodically (every 2s) so chat notifications
            // fire even while idle — without this, drain_sync_events() only runs
            // inside prepare_stream_context() which never happens when idle.
            if current_ms.saturating_sub(self.last_chat_drain_ms) >= 2_000 {
                self.last_chat_drain_ms = current_ms;
                crate::app::panels::refresh_all_panels(&mut self.state);
            }
            super::watchers::check_timer_based_deprecation(self);
            super::tools::pipeline::handle_tool_execution(self, ch.tx);
            super::streaming::finalize_stream(self);
            self.check_spine(ch.tx);
            self.check_my_turn_threads();
            super::streaming::process_api_check_results(self);

            // === REVERIE (CONTEXT OPTIMIZER SUB-AGENT) ===
            // Check if a reverie needs to start streaming (state.reverie exists but no stream yet)
            super::reverie::maybe_start_reverie_stream(self);
            // Poll reverie stream events (text chunks, tool calls, done/error)
            super::reverie::process_reverie_events(self);
            // Execute pending reverie tool calls (after main tools — main AI has priority)
            super::reverie::handle_reverie_tools(self);
            // Check if reverie ended without calling Report (auto-relaunch guard rail)
            super::reverie::check_reverie_end_turn(self);

            // Check if TUI reload was requested (by system_reload tool)
            if self.state.flags.lifecycle.reload_pending {
                self.writer.flush();
                save_state(&self.state);
                // Write reload flag AFTER save_state — otherwise save_state
                // overwrites config.json with reload_requested: false.
                crate::infra::tools::write_reload_flag();
                break;
            }

            // Check ownership periodically (every 1 second)
            if current_ms.saturating_sub(self.last_ownership_check_ms) >= 1000 {
                self.last_ownership_check_ms = current_ms;
                if !check_ownership() {
                    // Another instance took over - exit gracefully
                    break;
                }
            }

            // Update spinner animation if there's active loading/streaming
            self.update_spinner_animation();

            // Render if dirty and enough time has passed (capped at ~28fps)
            if self.state.flags.ui.dirty && current_ms.saturating_sub(self.last_render_ms) >= RENDER_THROTTLE_MS {
                let _r = terminal.draw(|frame| {
                    ui::render(frame, &mut self.state);
                    self.command_palette.render(frame, &self.state);
                })?;
                self.state.flags.ui.dirty = false;
                self.last_render_ms = current_ms;
            }

            // Adaptive poll: sleep longer when idle, shorter when actively streaming
            let poll_ms = if self.state.flags.stream.phase.is_streaming() || self.state.flags.ui.dirty {
                EVENT_POLL_MS // 8ms — responsive during streaming/active updates
            } else {
                50 // 50ms when idle — still responsive for typing, much less CPU
            };
            let _r = event::poll(Duration::from_millis(poll_ms))?;
        }

        Ok(())
    }

    /// Dispatch an `Action` through `apply_action` and handle the resulting side-effects.
    fn handle_action(&mut self, action: Action, tx: &Sender<StreamEvent>) {
        // Any action triggers a re-render
        self.state.flags.ui.dirty = true;
        match apply_action(&mut self.state, action) {
            ActionResult::StopStream => {
                self.typewriter.reset();
                self.pending_done = None;
                self.pending_tools.clear();

                // Flush any pending blocking tool results as "interrupted" so their
                // tool_use messages are properly paired with a tool_result.
                // Without this, the orphaned tool_use causes API 400 errors on
                // the next stream (tool_use without matching tool_result).
                super::tools::cleanup::flush_pending_tool_results_as_interrupted(self);

                // Pause auto-continuation when user explicitly cancels streaming.
                // Without this, the spine would immediately relaunch a new stream
                // (e.g., due to continue_until_todos_done), making the system
                // uncontrollable — the user can never stop it with Esc. (#44)
                // We set user_stopped instead of disabling continue_until_todos_done,
                // so auto-continuation resumes when the user sends a new message.
                // Notify all modules that the user stopped streaming
                for module in crate::modules::all_modules() {
                    module.on_stream_stop(&mut self.state);
                }
                self.state.touch_panel(Kind::SPINE);
                if let Some(msg) = self.state.messages.last()
                    && msg.role == "assistant"
                {
                    self.save_message_async(msg);
                }
                self.save_state_async();
            }
            ActionResult::Save => {
                self.save_state_async();
                // Check spine synchronously for responsive auto-continuation
                self.check_spine(tx);
            }
            ActionResult::SaveMessage(id) => {
                if let Some(msg) = self.state.messages.iter().find(|m| m.id == id) {
                    self.save_message_async(msg);
                }
                self.save_state_async();
            }
            ActionResult::StartApiCheck => {
                let (api_tx, api_rx) = std::sync::mpsc::channel();
                self.api_check_rx = Some(api_rx);
                crate::llms::start_api_check(self.state.llm_provider, self.state.current_model(), api_tx);
                self.save_state_async();
            }
            ActionResult::Nothing => {}
        }
    }

    /// Check the spine for auto-continuation decisions.
    /// Evaluates guard rails and auto-continuation logic.
    /// If a continuation fires, starts streaming.
    fn check_spine(&mut self, tx: &Sender<StreamEvent>) {
        // Check if incomplete todos should trigger auto-continuation
        self.check_todo_continuation();

        match check_spine(&mut self.state) {
            SpineDecision::Idle => {}
            SpineDecision::Blocked(reason) => {
                // Guard rail blocked — notification already created by engine.
                // Only mark dirty and save if this is a NEW block reason, to avoid
                // burning CPU/disk on every tick (~125/sec) when persistently blocked.
                if self.state.guard_rail_blocked.as_ref() != Some(&reason) {
                    self.state.guard_rail_blocked = Some(reason);
                    self.state.flags.ui.dirty = true;
                    self.save_state_async();
                }
            }
            SpineDecision::Continue(action) => {
                // Auto-continuation fired — apply it and start streaming
                self.state.guard_rail_blocked = None;
                let should_stream = apply_continuation(&mut self.state, action);
                if should_stream {
                    // Auto-Read: if unfocused with a MY_TURN thread, inject a
                    // synthetic Read tool call so the AI starts with focus set
                    // and thread content visible — saves a full round-trip.
                    self.maybe_inject_auto_read();

                    self.typewriter.reset();
                    self.pending_tools.clear();
                    let ctx = prepare_stream_context(&mut self.state, false, None);
                    let system_prompt = get_active_agent_content(&self.state);
                    let params = build_stream_params(&self.state, ctx, Some(system_prompt));
                    start_streaming(params, tx.clone());
                    self.save_state_async();
                    self.state.flags.ui.dirty = true;
                }
            }
        }
    }

    /// Check if todos need auto-continuation. Creates a single deduplicated
    /// notification — the spine's normal flow handles the rest.
    fn check_todo_continuation(&mut self) {
        if !SpineState::get(&self.state).config.continue_until_todos_done {
            return;
        }
        if self.state.flags.stream.phase.is_streaming() {
            return;
        }
        // Deduplicate: don't create if one already exists unprocessed
        let already = SpineState::get(&self.state)
            .notifications
            .iter()
            .any(|n| !n.is_processed() && n.source == "todo_continuation");
        if already {
            return;
        }
        let ts = cp_mod_todo::types::TodoState::get(&self.state);
        if !ts.has_incomplete_todos() {
            return;
        }
        let summary = ts.incomplete_todos_summary();
        let _r = SpineState::create_notification(
            &mut self.state,
            NotificationType::Custom,
            "todo_continuation".to_string(),
            format!("{} todo(s) remaining: {}", summary.len(), summary.join(", ")),
        );
    }

    /// Tick dirty flag so time-based spinners re-render.
    /// Throttled to 10fps (100ms) to avoid unnecessary re-renders.
    fn update_spinner_animation(&mut self) {
        let now = now_ms();
        if now.saturating_sub(self.last_spinner_ms) < 100 {
            return;
        }
        self.last_spinner_ms = now;
        self.state.flags.ui.dirty = true;
    }

    /// Inject a synthetic `Read` tool call when auto-continuation fires
    /// for a thread notification while the AI is unfocused.
    ///
    /// The Read is 100% deterministic in this scenario — the AI would
    /// always call it anyway. Injecting it saves a full round-trip:
    /// the AI starts streaming with focus already set and thread content
    /// visible, so it can immediately `Send` its response.
    ///
    /// Thread selection priority:
    /// 1. Extract thread IDs from the synthetic message (notification content)
    ///    and pick the first one that's still `MY_TURN`.
    /// 2. Fall back to any `MY_TURN` thread if no notification thread ID matches.
    ///
    /// Modifies the message list by popping the empty streaming-target
    /// assistant message, inserting the Read `tool_use` + `tool_result` pair,
    /// then pushing a new empty assistant for streaming.
    fn maybe_inject_auto_read(&mut self) {
        // Only inject when unfocused + a MY_TURN thread exists.
        let fs = FocusState::get(&self.state);
        if fs.focused_thread_id.is_some() {
            return;
        }

        // Extract thread IDs from the synthetic message that triggered this
        // continuation. The synthetic is the second-to-last message (before
        // the empty assistant streaming target pushed by `apply_continuation`).
        let candidate_ids: Vec<String> = self
            .state
            .messages
            .len()
            .checked_sub(2)
            .and_then(|idx| self.state.messages.get(idx))
            .filter(|m| m.role == "user" && m.content.starts_with("/* Auto-continuation:"))
            .map(|m| extract_thread_ids(&m.content))
            .unwrap_or_default();

        let ts = ThreadsState::get(&self.state);

        // Prefer the thread the notification is about; fall back to any MY_TURN.
        let my_turn = candidate_ids
            .iter()
            .find_map(|tid| {
                ts.threads
                    .iter()
                    .find(|t| t.id == *tid && t.status == ThreadStatus::MyTurn)
            })
            .or_else(|| ts.threads.iter().find(|t| t.status == ThreadStatus::MyTurn));

        let Some(thread) = my_turn else {
            return;
        };
        let tid = thread.id.clone();

        // Pop the empty assistant (streaming target) — we'll push a fresh
        // one after the injected Read messages.
        let Some(streaming_target) = self.state.messages.pop() else {
            return;
        };

        // Build a synthetic ToolUse for Read.
        let tool_use_id = format!("auto_read_{tid}");
        let input = serde_json::json!({
            "thread_id": tid,
            "intent": "Focus on thread",
            "verb": "Reading",
        });

        let tool_use = ToolUse {
            id: tool_use_id.clone(),
            name: "Read".into(),
            input: input.clone(),
        };

        // Execute Read — this sets focus and returns formatted messages.
        let result = cp_mod_threads::tools::execute_read(&tool_use, &mut self.state);

        // Create assistant message carrying the tool_use record.
        let tool_call_msg = crate::state::Message {
            id: format!("T{}", self.state.next_tool_id),
            uid: Some(format!("UID_{}_T", self.state.global_next_uid)),
            role: "assistant".into(),
            content: String::new(),
            msg_type: MsgKind::ToolCall,
            status: MsgStatus::Full,
            tool_uses: vec![ToolUseRecord {
                id: tool_use_id.clone(),
                name: "Read".into(),
                input,
            }],
            tool_results: vec![],
            input_tokens: 0,
            content_token_count: 0,
            timestamp_ms: now_ms(),
        };
        self.state.next_tool_id = self.state.next_tool_id.saturating_add(1);
        self.state.global_next_uid = self.state.global_next_uid.saturating_add(1);

        // Create tool_result message (user role).
        let result_msg = crate::state::Message {
            id: format!("R{}", self.state.next_result_id),
            uid: Some(format!("UID_{}_R", self.state.global_next_uid)),
            role: "user".into(),
            content: String::new(),
            msg_type: MsgKind::ToolResult,
            status: MsgStatus::Full,
            tool_uses: vec![],
            tool_results: vec![ToolResultRecord {
                tool_use_id,
                content: result.content,
                display: None,
                tldr: None,
                is_error: result.is_error,
                tool_name: "Read".into(),
            }],
            input_tokens: 0,
            content_token_count: 0,
            timestamp_ms: now_ms(),
        };
        self.state.next_result_id = self.state.next_result_id.saturating_add(1);
        self.state.global_next_uid = self.state.global_next_uid.saturating_add(1);

        // Persist both injected messages.
        self.save_message_async(&tool_call_msg);
        self.save_message_async(&result_msg);

        // Push: tool_call → tool_result → streaming target.
        self.state.messages.push(tool_call_msg);
        self.state.messages.push(result_msg);
        self.state.messages.push(streaming_target);
    }

    /// Notify when idle and a thread has `MY_TURN` status.
    ///
    /// Debounced via `FocusState::notified_my_turn_id` — fires once per
    /// thread transition to `MY_TURN`, cleared when the AI sends a reply
    /// (which sets `THEIR_TURN`).
    fn check_my_turn_threads(&mut self) {
        if self.state.flags.stream.phase.is_streaming() {
            return;
        }

        let threads = ThreadsState::get(&self.state);
        let my_turn = threads.threads.iter().find(|t| t.status == ThreadStatus::MyTurn);

        let Some(thread) = my_turn else {
            // No MY_TURN threads — clear debounce.
            FocusState::get_mut(&mut self.state).notified_my_turn_id = None;
            return;
        };

        let tid = thread.id.clone();
        let tname = thread.name.clone();

        // Debounce: already notified about this exact thread.
        // Re-fire only when the previous notification was consumed (processed)
        // but the AI still hasn't addressed the thread — creating a persistent
        // nudge loop until the thread is actually handled.
        if FocusState::get(&self.state).notified_my_turn_id.as_deref() == Some(&tid) {
            let has_unprocessed = SpineState::get(&self.state)
                .notifications
                .iter()
                .any(|n| !n.is_processed() && n.source == "my_turn_thread");
            if has_unprocessed {
                return; // Previous nudge still pending — don't spam
            }
            // Previous nudge consumed but thread still MY_TURN — clear debounce to re-fire
        }

        FocusState::get_mut(&mut self.state).notified_my_turn_id = Some(tid.clone());

        let content = format!(
            "Thread \"{tname}\" ({tid}) is MY_TURN — it has user input awaiting your response.\n\
             Use Read(thread_id=\"{tid}\") to see the conversation and respond.",
        );
        let _r = SpineState::create_notification(
            &mut self.state,
            NotificationType::Custom,
            "my_turn_thread".to_string(),
            content,
        );
    }
}

/// Extract thread IDs from notification content embedded in a synthetic message.
///
/// Looks for `thread_id="T..."` patterns (produced by thread input routing
/// and `check_my_turn_threads`). Returns all matches in order so the caller
/// can pick the first one that's still `MY_TURN`.
fn extract_thread_ids(content: &str) -> Vec<String> {
    let marker = "thread_id=\"";
    let mut ids = Vec::new();
    let mut search_from: usize = 0;
    while let Some(pos) = content.get(search_from..).and_then(|s| s.find(marker)) {
        let Some(start) = search_from
            .checked_add(pos)
            .and_then(|v| v.checked_add(marker.len()))
        else {
            break;
        };
        if let Some(end_offset) = content.get(start..).and_then(|s| s.find('"')) {
            if let Some(id_str) = start.checked_add(end_offset).and_then(|end| content.get(start..end)) {
                ids.push(id_str.to_string());
            }
            search_from = start.saturating_add(end_offset).saturating_add(1);
        } else {
            break;
        }
    }
    ids
}
