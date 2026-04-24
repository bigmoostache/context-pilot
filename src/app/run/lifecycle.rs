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
use cp_mod_spine::engine::{SpineDecision, apply_continuation, check_spine};
use cp_mod_spine::types::{NotificationType, SpineState};

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

                // Handle question form events if form is active (mutates state directly)
                if let Some(form) = self.state.get_ext::<cp_base::ui::question_form::PendingForm>()
                    && !form.resolved
                {
                    self.handle_question_form_event(&evt);
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
            // Check if a question form has been resolved by the user
            super::tools::checks::check_question_form(self, ch.tx);
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

    /// Update spinner animation frame if there's active loading/streaming.
    /// Throttled to 10fps (100ms) to avoid unnecessary re-renders.
    fn update_spinner_animation(&mut self) {
        let now = now_ms();
        if now.saturating_sub(self.last_spinner_ms) < 100 {
            return;
        }

        // Check if there's any active operation that needs spinner animation
        let has_active_spinner = self.state.flags.stream.phase.is_streaming()
            || self.state.flags.lifecycle.api_check_in_progress
            || self.state.context.iter().any(|c| c.cached_content.is_none() && c.context_type.needs_cache());

        if has_active_spinner {
            self.last_spinner_ms = now;
            // Increment spinner frame (wraps around automatically with u64)
            self.state.spinner_frame = self.state.spinner_frame.wrapping_add(1);
            // Mark dirty to trigger re-render with new spinner frame
            self.state.flags.ui.dirty = true;
        }
    }
}
