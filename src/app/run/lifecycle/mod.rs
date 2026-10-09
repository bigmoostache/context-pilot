use cp_base::state::data::model_helpers::ModelPricing as _;
use std::io;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::event;
use ratatui::prelude::{CrosstermBackend, Terminal};

use crate::app::actions::{Action, ActionResult, apply_action};
use crate::app::panels::now_ms;
use crate::infra::constants::{EVENT_POLL_MS, FULL_REDRAW_MS, RENDER_THROTTLE_MS};
use crate::state::Kind;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::{check_ownership, save_state};
use crate::ui;

use crate::app::App;

/// Spinner-animation redraw cadence (extracted for the 500-line cap).
mod animation;
/// Fleet-shared coucou delivery (once per tick) + legacy per-thread migration.
mod coucous;
/// Background-thread advancement: execute each non-focused active thread for
/// one step of the shared pipeline (Phase C). No-op at N=1.
mod fleet;
/// Fleet lifecycle I/O (Phase F): console orphan-prune, N-thread save, hard-delete
/// teardown, Errored re-engage. Split from `fleet` for the 500-line cap.
mod fleet_lifecycle;
/// The `loop.input` phase: event poll/read/route, every step profile-guarded.
mod input_phase;

/// Spine step body (spine check + background threads), with per-call timers.
mod spine_phase;
/// Per-thread stream runtime (typewriter/pending-tools/pending-done/…), stored
/// in each thread's module data so one thread's in-flight stream never bleeds
/// into another's.
pub(crate) mod stream_runtime;
use cp_mod_spine::engine::{SpineDecision, apply_continuation, check_spine};
use cp_mod_spine::types::{NotificationType, SpineState};

/// Bundles the I/O channels polled by the main event loop.
pub(crate) struct EventChannels<'ch> {
    /// Receives cache update results from the background hasher.
    pub cache_rx: &'ch Receiver<CacheUpdate>,
}

/// Outcome of the input phase, telling the main loop how to proceed this tick.
enum InputOutcome {
    /// Input fully handled + rendered — restart the loop (skip background work).
    Restart,
    /// User quit — break the loop.
    Quit,
    /// No short-circuit — fall through to background processing.
    Continue,
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

        // Boot-load every background thread's persisted per-thread context into
        // the fleet registry before the loop starts reconciling (F1 boot half).
        // No-op at N=1 (no per-thread files) — byte-identical boot.
        self.load_background_threads();

        // Kill console sessions on the server that belong to no loaded thread —
        // ONCE over the union of every thread's session keys (the per-thread
        // kill was removed from the console module: it would cross-kill other
        // threads' live sessions, F6/S2). N=1-identical: union = focused keys.
        self.prune_orphaned_console_sessions();

        // Coucous moved from per-thread watcher registries to one fleet-shared
        // registry: fold any legacy per-thread records in, once.
        self.migrate_legacy_coucous();

        // Start the interactive main-loop watchdog (purely observational — dumps
        // a diagnostic to .context-pilot/errors/ if the single-threaded loop
        // wedges, never terminates/signals the process). Idempotent.
        super::tools::watchdog::spawn();

        self.auto_resume_stream_if_flagged();

        // `--measure N`: force-enable perf monitoring from the first loop so
        // every substep's timing accumulates (bypasses the F12 toggle).
        ui::perf::PERF.enable_if_measuring();

        loop {
            let current_ms = now_ms();
            let _fg = cp_base::flame!("loop");

            // Main-loop heartbeat: a fresh tick every iteration. The watchdog
            // thread declares a wedge if this stops advancing (the loop ticks at
            // least every ~50 ms even while idle, so staleness is unambiguous).
            super::tools::watchdog::beat();
            super::tools::watchdog::mark(super::tools::watchdog::Step::Input);

            match self.handle_input_phase(terminal, current_ms)? {
                InputOutcome::Restart => continue,
                InputOutcome::Quit => break,
                InputOutcome::Continue => {}
            }

            self.run_background_phase(ch, current_ms);

            // Check if TUI reload was requested (by system_reload tool)
            if self.state.flags.lifecycle.reload_pending {
                self.writer.flush();
                self.save_all_threads();
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

            self.render_if_due(terminal, current_ms)?;

            super::tools::watchdog::mark(super::tools::watchdog::Step::Idle);

            // `--measure N`: once N iterations are recorded, dump the HTML
            // loop-profile report and exit cleanly (no further poll/park).
            if ui::perf::PERF.tick_measure() {
                self.writer.flush();
                break;
            }

            // Handed to the next input phase, which then skips its own
            // `poll(ZERO)` — the same question asked microseconds later.
            self.input_ready = Some(event::poll(Duration::from_millis(self.compute_poll_ms()))?);
        }

        Ok(())
    }

    /// Auto-resume streaming if the reload flag was set (e.g., after `reload_tui`).
    fn auto_resume_stream_if_flagged(&mut self) {
        if !self.resume_stream {
            return;
        }
        self.resume_stream = false;
        let _r = SpineState::create_notification(
            &mut self.state,
            NotificationType::ReloadResume,
            "reload_resume".to_owned(),
            "Resuming after TUI reload".to_owned(),
        );
        save_state(&self.state);
    }

    /// Loop-tail render: a forced full repaint once per [`FULL_REDRAW_MS`]
    /// (every cell rewritten in place so resize leftovers and stray escape
    /// output get overwritten), else a throttled diff render when dirty.
    ///
    /// The full repaint never clears the screen: a clear shows a blank frame
    /// before the redraw lands, which flickers. Instead the back buffer is
    /// poisoned so ratatui's diff emits every cell, and the write is wrapped in
    /// a synchronized update so the terminal presents it atomically.
    fn render_if_due(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        current_ms: u64,
    ) -> io::Result<()> {
        let full = current_ms.saturating_sub(self.last_full_redraw_ms) >= FULL_REDRAW_MS;
        let throttled_dirty =
            self.state.flags.ui.dirty && current_ms.saturating_sub(self.last_render_ms) >= RENDER_THROTTLE_MS;
        if !full && !throttled_dirty {
            return Ok(());
        }
        super::tools::watchdog::mark(super::tools::watchdog::Step::Render);
        if !full {
            return self.render_frame(terminal, current_ms);
        }
        self.last_full_redraw_ms = current_ms;
        {
            let _guard = crate::profile!("full_redraw_poison");
            poison_back_buffer(terminal);
        }
        crossterm::execute!(terminal.backend_mut(), crossterm::terminal::BeginSynchronizedUpdate)?;
        let result = self.render_frame(terminal, current_ms);
        crossterm::execute!(terminal.backend_mut(), crossterm::terminal::EndSynchronizedUpdate)?;
        result
    }

    /// Draw one frame: render the UI + command palette, clear dirty, stamp render time.
    fn render_frame(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        current_ms: u64,
    ) -> io::Result<()> {
        // G3 render-scoped drill-in: if the human has drilled into another
        // thread, make it the executing one just for this paint, then restore.
        // Focus and scheduling are untouched (Model 2).
        let drilled = {
            let _guard = crate::profile!("drill_in");
            self.take_drilled_runtime_for_render()
        };
        let draw_result = {
            let _guard = crate::profile!("terminal_draw");
            self.draw_timed(terminal)
        };
        {
            let _guard = crate::profile!("drill_restore");
            self.restore_drilled_runtime_after_render(drilled);
        }
        draw_result?;
        self.state.flags.ui.dirty = false;
        self.last_render_ms = current_ms;
        Ok(())
    }

    /// `Terminal::draw` inlined so each stage gets its own span: resize check,
    /// widget build (`ui_render`), buffer diff written to the backend, then the
    /// stdout flush — the last two can block on a slow terminal.
    ///
    /// Same steps as ratatui's `try_draw`. The app never sets a frame cursor,
    /// so the cursor is always hidden, as `draw` does for `None`.
    fn draw_timed(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
        {
            let _g = crate::profile!("term_autoresize");
            terminal.autoresize()?;
        }
        {
            let mut frame = terminal.get_frame();
            ui::render(&mut frame, &mut self.state);
            let _palette = crate::profile!("command_palette");
            self.command_palette.render(&mut frame, &self.state);
        }
        {
            let _g = crate::profile!("term_diff_write");
            terminal.flush()?;
        }
        let _g = crate::profile!("term_stdout_flush");
        terminal.hide_cursor()?;
        terminal.swap_buffers();
        io::Write::flush(terminal.backend_mut())
    }

    /// Adaptive poll interval: short while streaming/active or bridge-driven,
    /// long when idle — keeps latency low without pinning a core at rest.
    fn compute_poll_ms(&self) -> u64 {
        if self.state.thread().stream.phase.is_streaming() || self.state.flags.ui.dirty {
            EVENT_POLL_MS // 8ms — responsive during streaming/active updates
        } else if super::threads::bridge_active(&self.state) {
            2 // bridge-active idle — keep web command→apply latency ≤ a few ms
        } else {
            50 // 50ms when idle — still responsive for typing, much less CPU
        }
    }

    /// Run all background processing for one tick: bridge, threads, stream,
    /// cache, watchers, tools, spine, reverie. Linear pipeline, no branching.
    fn run_background_phase(&mut self, ch: &EventChannels<'_>, current_ms: u64) {
        super::tools::watchdog::mark(super::tools::watchdog::Step::Bridge);
        super::threads::poll_bridge_commands(self);
        super::tools::watchdog::mark(super::tools::watchdog::Step::ThreadsEmit);
        super::threads::emit_bridge_deltas(self);
        // Make the focused thread the executing one: if focus changed since
        // last tick (agent `Read`, human drill-in), register the old one for
        // background stepping and execute the new one, so the focused pipeline
        // below operates on the correct thread and its stream frames are tagged
        // with its id. No-op when focus is unchanged.
        {
            let _guard = crate::profile!("follow_focus");
            self.follow_focus();
        }
        super::tools::watchdog::mark(super::tools::watchdog::Step::Stream);
        super::streaming::process_stream_events(self);
        super::streaming::handle_retry(self);
        super::streaming::process_typewriter(self);
        super::tools::watchdog::mark(super::tools::watchdog::Step::Cache);
        super::watchers::process_cache_updates(self, ch.cache_rx);
        super::tools::watchdog::mark(super::tools::watchdog::Step::Watchers);
        super::watchers::process_watcher_events(self);
        // Check if we're waiting for panels and they're ready (non-blocking)
        super::tools::checks::check_waiting_for_panels(self);
        // Check if deferred sleep timer has expired (non-blocking)
        super::tools::checks::check_deferred_sleep(self);
        // Check watchers (blocking sentinel replacement + async → spine notifications)
        super::tools::cleanup::check_watchers(self);
        // Fleet-shared coucous: polled once per tick for every thread, with the
        // focused thread executing (background steps never see them).
        self.check_coucous();
        self.recover_bridge_if_pending(current_ms);
        self.drain_chat_sync_if_due(current_ms);
        super::watchers::check_timer_based_deprecation(self);
        super::tools::watchdog::mark(super::tools::watchdog::Step::Tools);
        super::tools::pipeline::handle_tool_execution(self);
        // Snapshot "mid-turn this tick" BEFORE `finalize_stream` — it applies
        // the turn's `pending_done` and flips the phase to `Idle`, so reading
        // the phase AFTER it (inside the hook) would almost always see `Idle`
        // mid-turn and wrongly take the idle auto-read branch instead of the
        // inline streaming push.
        let was_streaming = self.state.thread().stream.phase.is_streaming();
        super::streaming::finalize_stream(self);
        // Incoming-message behavior on the focused thread: inline push while
        // streaming, idle auto-read otherwise. Runs before the spine check so an
        // idle auto-read's continuation nudge is picked up this same tick.
        super::threads::handle_incoming_focused_messages(self, was_streaming);
        cp_mod_threads::types::FocusState::tick_read_dwell(&mut self.state, current_ms);
        super::tools::watchdog::mark(super::tools::watchdog::Step::Spine);
        self.run_spine_phase(current_ms);

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
    }

    /// Bridge self-heal: if `CP_BRIDGE=1` but boot lost the flock race on a fast
    /// relaunch, the bridge sits PENDING and the agent is silently unreachable to
    /// web sends. Retry boot every ~2s — a fail-fast, non-blocking attempt that
    /// becomes a no-op the instant the bridge is live (or was never pending).
    fn recover_bridge_if_pending(&mut self, current_ms: u64) {
        if current_ms.saturating_sub(self.last_bridge_recover_ms) >= 2_000 {
            self.last_bridge_recover_ms = current_ms;
            cp_mod_bridge::try_recover(&mut self.state);
        }
    }

    /// Drain Matrix sync events periodically (every 2s) so chat notifications
    /// fire even while idle — without this, `drain_sync_events()` only runs
    /// inside `prepare_stream_context()`, which never happens when idle.
    fn drain_chat_sync_if_due(&mut self, current_ms: u64) {
        if current_ms.saturating_sub(self.last_chat_drain_ms) >= 2_000 {
            self.last_chat_drain_ms = current_ms;
            super::tools::watchdog::mark(super::tools::watchdog::Step::PanelRefresh);
            crate::app::panels::refresh_all_panels(&mut self.state);
        }
    }

    /// Dispatch an `Action` through `apply_action` and handle the resulting side-effects.
    fn handle_action(&mut self, action: Action) {
        self.state.flags.ui.dirty = true; // any action triggers a re-render
        // `if let` (not an exhaustive match) so ActionResult stays #[non_exhaustive].
        // SaveMessage is the only payload-bearing variant; the fieldless rest dispatch below.
        let result = {
            let _g = crate::profile!("apply_action");
            let _v = crate::infra::profiler::variant_span(&action);
            apply_action(&mut self.state, action)
        };
        if let ActionResult::SaveMessage(id) = result {
            let _g = crate::profile!("save_message_by_id");
            self.save_message_by_id(&id);
        } else {
            self.handle_fieldless_result(&result);
        }
    }

    /// Handle the fieldless [`ActionResult`] variants (everything except
    /// `SaveMessage`). The trailing `else` absorbs `Nothing` plus any future
    /// `non_exhaustive` variant.
    fn handle_fieldless_result(&mut self, result: &ActionResult) {
        if matches!(result, ActionResult::StopStream) {
            self.on_stop_stream();
        } else if matches!(result, ActionResult::Save) {
            self.save_state_async();
            let _g = crate::profile!("check_spine");
            self.check_spine(); // synchronous for responsive auto-continuation
        } else if matches!(result, ActionResult::StartApiCheck) {
            self.start_api_check_now();
        } else {
            // Nothing + future non_exhaustive variants: no side-effect.
        }
    }

    /// Persist the message with the given display `id` (if it still exists) plus
    /// the full state — the `ActionResult::SaveMessage` side-effect.
    fn save_message_by_id(&self, id: &str) {
        if let Some(msg) = self.state.thread().messages.iter().find(|m| m.id == id) {
            self.save_message_async(msg);
        }
        self.save_state_async();
    }

    /// Kick off an async API connectivity check for the current provider/model and
    /// persist — the `ActionResult::StartApiCheck` side-effect.
    fn start_api_check_now(&mut self) {
        let (api_tx, api_rx) = std::sync::mpsc::channel();
        self.api_check_rx = Some(api_rx);
        crate::llms::start_api_check(self.state.llm_provider, self.state.current_model(), api_tx);
        self.save_state_async();
    }

    /// Side-effects of an [`ActionResult::StopStream`]: reset the typewriter, drop
    /// pending work, flush orphaned blocking tool results as interrupted (so every
    /// `tool_use` stays paired and the next stream avoids an API 400), notify modules,
    /// persist. Esc's auto-continuation pause lives in `apply_action`'s `user_stopped`
    /// flag — without it the spine would instantly relaunch a stream, making Esc
    /// uncancellable (#44).
    fn on_stop_stream(&mut self) {
        self.stream_rt_mut().typewriter.reset();
        self.stream_rt_mut().pending_done = None;
        self.stream_rt_mut().pending_tools.clear();
        super::tools::cleanup::flush_pending_tool_results_as_interrupted(self);
        for module in crate::modules::all_modules() {
            module.on_stream_stop(&mut self.state);
        }
        self.state.touch_panel(Kind::SPINE);
        if let Some(msg) = self.state.thread().messages.last()
            && msg.role == "assistant"
        {
            self.save_message_async(msg);
        }
        self.save_state_async();
    }

    /// Check the spine for auto-continuation decisions.
    /// Evaluates guard rails and auto-continuation logic.
    /// If a continuation fires, starts streaming.
    pub(super) fn check_spine(&mut self) {
        // Idle is the implicit no-op tail — a non_exhaustive enum forbids a
        // cross-crate exhaustive match, so the two actionable variants are
        // handled via if-let and Idle simply falls through.
        let decision = check_spine(&mut self.state);
        if let SpineDecision::Blocked(reason) = decision {
            // Guard rail blocked — notification already created by engine.
            // Only mark dirty and save if this is a NEW block reason, to avoid
            // burning CPU/disk on every tick (~125/sec) when persistently blocked.
            if self.state.thread().guard_rail_blocked.as_ref() != Some(&reason) {
                self.state.thread_mut().guard_rail_blocked = Some(reason);
                self.state.flags.ui.dirty = true;
                self.save_state_async();
            }
        } else if let SpineDecision::Continue(action) = decision {
            // Auto-continuation fired — apply it and start streaming
            self.state.thread_mut().guard_rail_blocked = None;
            let should_stream = apply_continuation(&mut self.state, action);
            if should_stream {
                self.stream_rt_mut().typewriter.reset();
                self.stream_rt_mut().pending_tools.clear();
                crate::app::run::streaming::spawn_stream_with_context(self, false);
                self.save_state_async();
                self.state.flags.ui.dirty = true;
            }
        } else {
            // SpineDecision::Idle — no auto-continuation, nothing to do.
        }
    }
}

/// Make ratatui's next diff rewrite every cell without clearing the screen.
///
/// Fills the current (about to become previous) buffer with a background no
/// real frame uses, then swaps: the fresh frame differs from it everywhere.
fn poison_back_buffer(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    /// Sentinel background — any value the UI never paints works.
    const POISON: ratatui::style::Color = ratatui::style::Color::Rgb(1, 2, 3);
    for cell in &mut terminal.current_buffer_mut().content {
        let _cell = cell.set_bg(POISON);
    }
    terminal.swap_buffers();
}
