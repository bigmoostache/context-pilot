//! Daemon event loop — headless equivalent of [`lifecycle::run`].
//!
//! Polls a [`SocketServer`] for client input instead of a terminal,
//! builds IR frames and broadcasts them instead of rendering via ratatui.
//! All background processing (streaming, tools, spine, reverie) runs
//! identically to the terminal loop.

use std::io;
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::app::App;
use crate::app::actions::Action;
use crate::app::events::handle_event;
use crate::app::panels::now_ms;
use crate::headless::protocol::ClientMessage;
use crate::headless::server::SocketServer;
use crate::infra::constants::{EVENT_POLL_MS, RENDER_THROTTLE_MS};
use crate::state::persistence::{check_ownership, save_state};

use cp_mod_spine::types::{NotificationType, SpineState};

use super::lifecycle::EventChannels;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Daemon event loop: polls socket server, processes events, broadcasts IR frames.
    ///
    /// Structurally mirrors [`App::run`] but replaces terminal I/O with
    /// socket I/O. The daemon builds IR [`Frame`]s and broadcasts them to
    /// connected clients; input arrives as deserialized [`ClientMessage`]s.
    pub(crate) fn run_daemon(&mut self, server: &mut SocketServer, ch: &EventChannels<'_>) -> io::Result<()> {
        // ── Setup (same as terminal loop) ────────────────────────
        super::watchers::setup_file_watchers(self);
        super::watchers::sync_gh_watches(self);
        super::watchers::schedule_initial_cache_refreshes(self);
        save_state(&self.state);

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

            // ── INPUT: Accept new clients + drain events ─────────
            server.accept_pending();
            for client_event in server.poll_events() {
                match client_event.message {
                    ClientMessage::Input { event: ref evt } => {
                        self.process_daemon_input(evt, ch.tx);
                    }
                    ClientMessage::Attach { .. } => {
                        // Dimensions stored by server.poll_events(). Force
                        // a frame broadcast so the new client sees content.
                        self.state.flags.ui.dirty = true;
                    }
                    ClientMessage::Detach => {
                        // Server handles writer cleanup — nothing to do.
                    }
                    ClientMessage::Quit => {
                        self.writer.flush();
                        save_state(&self.state);
                        server.shutdown();
                        return Ok(());
                    }
                    ClientMessage::Ping => {}
                }
            }

            // ── BACKGROUND PROCESSING (identical to terminal) ────
            super::streaming::process_stream_events(self, ch.rx);
            super::streaming::handle_retry(self, ch.tx);
            super::streaming::process_typewriter(self);
            super::watchers::process_cache_updates(self, ch.cache_rx);
            super::watchers::process_watcher_events(self);
            super::tools::checks::check_waiting_for_panels(self, ch.tx);
            super::tools::checks::check_deferred_sleep(self, ch.tx);
            super::tools::checks::check_question_form(self, ch.tx);
            super::tools::cleanup::check_watchers(self, ch.tx);

            if current_ms.saturating_sub(self.last_gh_sync_ms) >= 5_000 {
                self.last_gh_sync_ms = current_ms;
                super::watchers::sync_gh_watches(self);
            }
            if current_ms.saturating_sub(self.last_chat_drain_ms) >= 2_000 {
                self.last_chat_drain_ms = current_ms;
                crate::app::panels::refresh_all_panels(&mut self.state);
            }
            super::watchers::check_timer_based_deprecation(self);
            super::tools::pipeline::handle_tool_execution(self, ch.tx);
            super::streaming::finalize_stream(self);
            self.check_spine(ch.tx);
            super::streaming::process_api_check_results(self);

            // ── REVERIE ──────────────────────────────────────────
            super::reverie::maybe_start_reverie_stream(self);
            super::reverie::process_reverie_events(self);
            super::reverie::handle_reverie_tools(self);
            super::reverie::check_reverie_end_turn(self);

            // ── RELOAD ───────────────────────────────────────────
            if self.state.flags.lifecycle.reload_pending {
                self.writer.flush();
                save_state(&self.state);
                crate::infra::tools::write_reload_flag();
                server.shutdown();
                break;
            }

            // ── OWNERSHIP CHECK ──────────────────────────────────
            if current_ms.saturating_sub(self.last_ownership_check_ms) >= 1000 {
                self.last_ownership_check_ms = current_ms;
                if !check_ownership() {
                    server.shutdown();
                    break;
                }
            }

            // ── SIGNAL CHECK ─────────────────────────────────────
            // SIGTERM / SIGINT set the DAEMON_SHUTDOWN flag — honour it
            if crate::headless::launch::is_shutdown_requested() {
                log::info!("headless: shutdown signal received, exiting gracefully");
                self.writer.flush();
                save_state(&self.state);
                server.shutdown();
                break;
            }

            self.update_spinner_animation();

            // ── RENDER: Build IR frame + broadcast ───────────────
            // Skip frame building entirely when no clients are connected
            // (design doc §6: "daemon skips frame building entirely").
            if self.state.flags.ui.dirty
                && current_ms.saturating_sub(self.last_render_ms) >= RENDER_THROTTLE_MS
                && server.client_count() > 0
            {
                let mut frame = crate::ui::ir::build_frame(&self.state);

                // Attach command palette overlay if open (palette lives
                // on App, not State, so build_frame can't include it).
                if self.command_palette.is_open {
                    frame.overlays.push(cp_render::conversation::Overlay::CommandPalette(self.command_palette.to_ir()));
                }

                let _ = server.broadcast_frame(&frame);
                self.state.flags.ui.dirty = false;
                self.last_render_ms = current_ms;
            }

            // ── ADAPTIVE SLEEP ───────────────────────────────────
            // No terminal to poll — sleep directly. Shorter during active
            // streaming for responsive frame updates.
            let poll_ms = if self.state.flags.stream.phase.is_streaming() || self.state.flags.ui.dirty {
                EVENT_POLL_MS
            } else {
                50
            };
            std::thread::sleep(Duration::from_millis(poll_ms));
        }

        Ok(())
    }

    /// Process a single input event from a connected client.
    ///
    /// Mirrors the input handling in [`App::run`]: routes through command
    /// palette, autocomplete, and question form before falling through to
    /// the normal event→action pipeline.
    fn process_daemon_input(&mut self, evt: &crossterm::event::Event, tx: &Sender<crate::infra::api::StreamEvent>) {
        self.state.flags.ui.dirty = true;

        // Command palette intercepts all input when open
        if self.command_palette.is_open {
            if let Some(action) = self.handle_palette_event(evt) {
                self.handle_action(action, tx);
            }
            return;
        }

        // Autocomplete popup intercepts when active
        if let Some(ac) = self.state.get_ext::<cp_base::state::autocomplete::Suggestions>()
            && ac.active
        {
            self.handle_autocomplete_event(evt);
            return;
        }

        // Question form intercepts when active
        if let Some(form) = self.state.get_ext::<cp_base::ui::question_form::PendingForm>()
            && !form.resolved
        {
            self.handle_question_form_event(evt);
            return;
        }

        // Normal event handling
        let Some(action) = handle_event(evt, &self.state) else {
            // handle_event returns None for quit (Ctrl+Q) — but in daemon
            // mode, Ctrl+Q is intercepted by the client as ClientMessage::Quit.
            // If we somehow get here, treat it as a no-op.
            return;
        };

        if matches!(action, Action::OpenCommandPalette) {
            self.command_palette.open(&self.state);
        } else {
            self.handle_action(action, tx);
        }
    }
}
