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
        super::watchers::schedule_initial_cache_refreshes(self);

        // Claim ownership immediately
        save_state(&self.state);

        // Start the interactive main-loop watchdog (purely observational — dumps
        // a diagnostic to .context-pilot/errors/ if the single-threaded loop
        // wedges, never terminates/signals the process). Idempotent.
        super::tools::watchdog::spawn();

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

            // Main-loop heartbeat: a fresh tick every iteration. The watchdog
            // thread declares a wedge if this stops advancing (the loop ticks at
            // least every ~50 ms even while idle, so staleness is unambiguous).
            super::tools::watchdog::beat();
            super::tools::watchdog::mark(super::tools::watchdog::Step::Input);

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
            super::tools::watchdog::mark(super::tools::watchdog::Step::Bridge);
            super::threads::poll_bridge_commands(self);
            super::tools::watchdog::mark(super::tools::watchdog::Step::ThreadsEmit);
            super::threads::emit_vitals(self);
            super::threads::emit_messages(self);
            super::threads::emit_thread_status(self);
            super::threads::emit_thread_focus(self);
            super::threads::emit_thread_archived(self);
            super::threads::emit_thread_paused(self);
            super::tools::watchdog::mark(super::tools::watchdog::Step::Stream);
            super::streaming::process_stream_events(self, ch.rx);
            super::streaming::handle_retry(self, ch.tx);
            super::streaming::process_typewriter(self);
            super::tools::watchdog::mark(super::tools::watchdog::Step::Cache);
            super::watchers::process_cache_updates(self, ch.cache_rx);
            super::tools::watchdog::mark(super::tools::watchdog::Step::Watchers);
            super::watchers::process_watcher_events(self);
            // Check if we're waiting for panels and they're ready (non-blocking)
            super::tools::checks::check_waiting_for_panels(self, ch.tx);
            // Check if deferred sleep timer has expired (non-blocking)
            super::tools::checks::check_deferred_sleep(self, ch.tx);
            // Check watchers (blocking sentinel replacement + async → spine notifications)
            super::tools::cleanup::check_watchers(self, ch.tx);
            // Bridge self-heal: if CP_BRIDGE=1 but boot lost the flock race on a
            // fast relaunch, the bridge sits PENDING and the agent is silently
            // unreachable to web sends. Retry boot every ~2s — a fail-fast,
            // non-blocking attempt that becomes a no-op the instant the bridge
            // is live (or was never pending). Self-heals the moment the dying
            // predecessor frees the lock, with no manual relaunch.
            if current_ms.saturating_sub(self.last_bridge_recover_ms) >= 2_000 {
                self.last_bridge_recover_ms = current_ms;
                cp_mod_bridge::try_recover(&mut self.state);
            }
            // Drain Matrix sync events periodically (every 2s) so chat notifications
            // fire even while idle — without this, drain_sync_events() only runs
            // inside prepare_stream_context() which never happens when idle.
            if current_ms.saturating_sub(self.last_chat_drain_ms) >= 2_000 {
                self.last_chat_drain_ms = current_ms;
                super::tools::watchdog::mark(super::tools::watchdog::Step::PanelRefresh);
                crate::app::panels::refresh_all_panels(&mut self.state);
            }
            super::watchers::check_timer_based_deprecation(self);
            super::tools::watchdog::mark(super::tools::watchdog::Step::Tools);
            super::tools::pipeline::handle_tool_execution(self, ch.tx);
            super::streaming::finalize_stream(self);
            super::tools::watchdog::mark(super::tools::watchdog::Step::Spine);
            self.check_spine(ch.tx);
            super::threads::check_my_turn_threads(self);
            super::streaming::process_api_check_results(self);

            // === REVERIE (CONTEXT OPTIMIZER SUB-AGENT) ===
            super::tools::watchdog::mark(super::tools::watchdog::Step::Reverie);
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
                super::tools::watchdog::mark(super::tools::watchdog::Step::Save);
                if !check_ownership() {
                    // Another instance took over - exit gracefully
                    break;
                }
            }

            // Update spinner animation if there's active loading/streaming
            self.update_spinner_animation();

            // Render if dirty and enough time has passed (capped at ~28fps)
            if self.state.flags.ui.dirty && current_ms.saturating_sub(self.last_render_ms) >= RENDER_THROTTLE_MS {
                super::tools::watchdog::mark(super::tools::watchdog::Step::Render);
                let _r = terminal.draw(|frame| {
                    ui::render(frame, &mut self.state);
                    self.command_palette.render(frame, &self.state);
                })?;
                self.state.flags.ui.dirty = false;
                self.last_render_ms = current_ms;
            }

            // Adaptive poll: sleep longer when idle, shorter when actively
            // streaming or when the orchestration bridge is connected (a web
            // UI is driving commands and expects sub-10ms apply latency, so we
            // service the bridge socket every couple of ms instead of every
            // 50ms — only while bridge-active, to keep idle CPU low otherwise).
            let poll_ms = if self.state.flags.stream.phase.is_streaming() || self.state.flags.ui.dirty {
                EVENT_POLL_MS // 8ms — responsive during streaming/active updates
            } else if super::threads::bridge_active(&self.state) {
                2 // bridge-active idle — keep web command→apply latency ≤ a few ms
            } else {
                50 // 50ms when idle — still responsive for typing, much less CPU
            };
            super::tools::watchdog::mark(super::tools::watchdog::Step::Idle);
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
                    // synthetic Read tool call. When injected, the Read rides the
                    // NORMAL tool pipeline (handle_tool_execution on the next
                    // tick) — which executes it, breaks tempo, and drives the
                    // follow-up stream via continue_streaming itself. So when it
                    // injected we must NOT start a stream here, nor clear
                    // pending_tools (that would wipe the injected Read).
                    // Behaviourally identical to the LLM emitting a Read as its
                    // first action (T322).
                    if !super::threads::maybe_inject_auto_read(self) {
                        self.typewriter.reset();
                        self.pending_tools.clear();
                        let ctx = prepare_stream_context(&mut self.state, false, None);
                        let system_prompt = get_active_agent_content(&self.state);
                        let params = build_stream_params(&self.state, ctx, Some(system_prompt));
                        start_streaming(params, tx.clone());
                    }
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
        // Report only the COUNT plus the FIRST incomplete item — never the full
        // list. On a large roadmap (hundreds of todos) dumping every remaining
        // entry floods the model with redundant tokens on every auto-continuation
        // tick; the count conveys the scale and the first item points at what to
        // pick up next, which is all the continuation nudge needs (T361).
        let summary = ts.incomplete_todos_summary();
        let first = summary.first().map_or("", String::as_str);
        let _r = SpineState::create_notification(
            &mut self.state,
            NotificationType::Custom,
            "todo_continuation".to_string(),
            format!("Non-completed todo items ({}). First: {first}", summary.len()),
        );
    }

    /// Tick the dirty flag at 10fps **only while something on-screen is
    /// actually animating**, so time-based spinners advance without pinning a
    /// core when the agent is idle.
    ///
    /// Previously this forced a full-frame redraw every 100ms unconditionally —
    /// a permanent 10fps re-render of the entire UI (sidebar + content + status
    /// bar IR rebuilt and diffed) that ran *forever*, even with nothing to
    /// animate, and was a primary source of the "idle yet pinning CPU"
    /// pathology (T309). The fix gates the forced redraw on
    /// [`has_active_animation`](Self::has_active_animation): a genuinely idle
    /// agent (READY badge, no loading panels, no running console) now produces
    /// **zero** periodic renders and falls to ~0% CPU, while every animated
    /// state (streaming/tooling, a timed-watcher WAITING badge, a loading
    /// panel, a running console) still ticks at the full 10fps. Event-driven
    /// redraws (input, stream chunks, cache updates, state mutations) are
    /// untouched — they set `dirty` at their source — so the screen still
    /// updates instantly on any real change.
    ///
    /// The 100ms throttle gates the *condition check itself* to 10Hz, so the
    /// (cheap) animation scan never runs at the loop's full poll cadence.
    fn update_spinner_animation(&mut self) {
        let now = now_ms();
        if now.saturating_sub(self.last_spinner_ms) < 100 {
            return;
        }
        self.last_spinner_ms = now;
        if Self::has_active_animation(&self.state) {
            self.state.flags.ui.dirty = true;
        }
    }

    /// Whether any on-screen element is currently animating and therefore needs
    /// the periodic [`update_spinner_animation`](Self::update_spinner_animation)
    /// redraw tick.
    ///
    /// Mirrors *exactly* the conditions under which the renderer draws a moving
    /// spinner, so the forced-redraw cadence is driven by — and only by — real
    /// animation:
    /// - **streaming / tooling** — the primary badge spins;
    /// - a **timed watcher** is pending — the `WAITING` badge (`AccentDim`)
    ///   spins;
    /// - a **panel is still loading** its first cache content — the `LOADING`
    ///   badge and the sidebar entry spin;
    /// - a **console is running** — its sidebar glyph spins.
    ///
    /// When none hold, the screen is static and no periodic redraw is needed.
    fn has_active_animation(state: &crate::state::State) -> bool {
        if state.flags.stream.phase.is_streaming() {
            return true; // STREAMING / TOOLING badge spinner
        }
        // A pending timed watcher renders the animated WAITING badge.
        let has_timed_watcher = state
            .get_ext::<cp_base::state::watchers::WatcherRegistry>()
            .is_some_and(|reg| reg.active_watchers().iter().any(|w| w.fire_at_ms().is_some()));
        if has_timed_watcher {
            return true;
        }
        // A panel still loading its first content (LOADING badge + sidebar
        // spinner) or a running console (animated sidebar glyph).
        state.context.iter().any(|c| {
            (c.cached_content.is_none() && c.context_type.needs_cache())
                || (c.context_type.as_str() == "console"
                    && c.get_meta_str("console_status").is_some_and(|s| s.starts_with("running")))
        })
    }
}
