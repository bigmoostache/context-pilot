use std::io;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crossterm::event;
use ratatui::prelude::*;

use crate::app::actions::{Action, ActionResult, apply_action};
use crate::app::events::handle_event;
use crate::app::panels::now_ms;
use crate::infra::api::{StreamEvent, StreamParams, start_streaming};
use crate::infra::constants::{DEFAULT_WORKER_ID, EVENT_POLL_MS, RENDER_THROTTLE_MS};
use crate::state::ContextType;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::{check_ownership, save_state};
use crate::ui;

use crate::app::App;
use crate::app::context::{get_active_agent_content, prepare_stream_context};
use cp_mod_spine::engine::{SpineDecision, apply_continuation, check_spine};
use cp_mod_spine::{NotificationType, SpineState};

impl App {
    pub(crate) fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        tx: &Sender<StreamEvent>,
        rx: &Receiver<StreamEvent>,
        cache_rx: &Receiver<CacheUpdate>,
    ) -> io::Result<()> {
        // Initial cache setup - watch files and schedule initial refreshes
        self.setup_file_watchers();
        self.sync_gh_watches();
        self.schedule_initial_cache_refreshes();

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
                        self.handle_action(action, tx);
                    }
                    self.state.dirty = true;

                    // Render immediately after input for instant feedback
                    if self.state.dirty {
                        let _r = terminal.draw(|frame| {
                            ui::render(frame, &mut self.state);
                            self.command_palette.render(frame, &self.state);
                        })?;
                        self.state.dirty = false;
                        self.last_render_ms = current_ms;
                    }
                    continue;
                }

                // Handle autocomplete events if popup is active
                if let Some(ac) = self.state.get_ext::<cp_base::autocomplete::AutocompleteState>()
                    && ac.active
                {
                    self.handle_autocomplete_event(&evt);
                    self.state.dirty = true;

                    // Render immediately
                    if self.state.dirty {
                        let _r = terminal.draw(|frame| {
                            ui::render(frame, &mut self.state);
                            self.command_palette.render(frame, &self.state);
                        })?;
                        self.state.dirty = false;
                        self.last_render_ms = current_ms;
                    }
                    continue;
                }

                // Handle question form events if form is active (mutates state directly)
                if let Some(form) = self.state.get_ext::<cp_base::ui::PendingQuestionForm>()
                    && !form.resolved
                {
                    self.handle_question_form_event(&evt);
                    self.state.dirty = true;

                    // Render immediately
                    if self.state.dirty {
                        let _r = terminal.draw(|frame| {
                            ui::render(frame, &mut self.state);
                            self.command_palette.render(frame, &self.state);
                        })?;
                        self.state.dirty = false;
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
                if let Action::OpenCommandPalette = action {
                    self.command_palette.open(&self.state);
                    self.state.dirty = true;
                } else {
                    self.handle_action(action, tx);
                }

                // Render immediately after input for instant feedback
                if self.state.dirty {
                    let _r = terminal.draw(|frame| {
                        ui::render(frame, &mut self.state);
                        self.command_palette.render(frame, &self.state);
                    })?;
                    self.state.dirty = false;
                    self.last_render_ms = current_ms;
                }
            }

            // === BACKGROUND PROCESSING ===
            self.process_stream_events(rx);
            self.handle_retry(tx);
            self.process_typewriter();
            self.process_cache_updates(cache_rx);
            self.process_watcher_events();
            // Check if we're waiting for panels and they're ready (non-blocking)
            self.check_waiting_for_panels(tx);
            // Check if deferred sleep timer has expired (non-blocking)
            self.check_deferred_sleep(tx);
            // Check if a question form has been resolved by the user
            self.check_question_form(tx);
            // Check watchers (blocking sentinel replacement + async → spine notifications)
            self.check_watchers(tx);
            // Throttle gh watcher sync to every 5 seconds (mutex lock + iteration)
            if current_ms.saturating_sub(self.last_gh_sync_ms) >= 5_000 {
                self.last_gh_sync_ms = current_ms;
                self.sync_gh_watches();
            }
            self.check_timer_based_deprecation();
            self.handle_tool_execution(tx);
            self.finalize_stream();
            self.check_spine(tx);
            self.process_api_check_results();

            // === REVERIE (CONTEXT OPTIMIZER SUB-AGENT) ===
            // Check if a reverie needs to start streaming (state.reverie exists but no stream yet)
            self.maybe_start_reverie_stream();
            // Poll reverie stream events (text chunks, tool calls, done/error)
            self.process_reverie_events();
            // Execute pending reverie tool calls (after main tools — main AI has priority)
            self.handle_reverie_tools();
            // Check if reverie ended without calling Report (auto-relaunch guard rail)
            self.check_reverie_end_turn();

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
            if self.state.dirty && current_ms.saturating_sub(self.last_render_ms) >= RENDER_THROTTLE_MS {
                let _r = terminal.draw(|frame| {
                    ui::render(frame, &mut self.state);
                    self.command_palette.render(frame, &self.state);
                })?;
                self.state.dirty = false;
                self.last_render_ms = current_ms;
            }

            // Adaptive poll: sleep longer when idle, shorter when actively streaming
            let poll_ms = if self.state.is_streaming || self.state.dirty {
                EVENT_POLL_MS // 8ms — responsive during streaming/active updates
            } else {
                50 // 50ms when idle — still responsive for typing, much less CPU
            };
            let _r = event::poll(Duration::from_millis(poll_ms))?;
        }

        Ok(())
    }

    fn handle_action(&mut self, action: Action, tx: &Sender<StreamEvent>) {
        // Any action triggers a re-render
        self.state.dirty = true;
        match apply_action(&mut self.state, action) {
            ActionResult::StopStream => {
                self.typewriter.reset();
                self.pending_done = None;
                self.pending_tools.clear();

                // Flush any pending blocking tool results as "interrupted" so their
                // tool_use messages are properly paired with a tool_result.
                // Without this, the orphaned tool_use causes API 400 errors on
                // the next stream (tool_use without matching tool_result).
                self.flush_pending_tool_results_as_interrupted();

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
                self.state.touch_panel(ContextType::SPINE);
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
                    self.state.dirty = true;
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
                    self.save_state_async();
                    self.state.dirty = true;
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
        if self.state.is_streaming {
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
        let ts = cp_mod_todo::TodoState::get(&self.state);
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
        let has_active_spinner = self.state.is_streaming
            || self.state.api_check_in_progress
            || self.state.context.iter().any(|c| c.cached_content.is_none() && c.context_type.needs_cache());

        if has_active_spinner {
            self.last_spinner_ms = now;
            // Increment spinner frame (wraps around automatically with u64)
            self.state.spinner_frame = self.state.spinner_frame.wrapping_add(1);
            // Mark dirty to trigger re-render with new spinner frame
            self.state.dirty = true;
        }
    }
}
