//! The main loop's input phase (`loop.input`), split from `lifecycle/mod.rs`.
//!
//! Every statement that costs time sits under a named profile guard, so the
//! `loop.input.*` children sum to `loop.input` with no uncovered remainder.
//! Only bool tests and early returns run outside a guard.

use std::io;
use std::time::Duration;

use crossterm::event;
use ratatui::prelude::{CrosstermBackend, Terminal};

use crate::app::App;
use crate::app::actions::Action;
use crate::app::events::handle_event;
use crate::infra::constants::RENDER_THROTTLE_MS;

use super::InputOutcome;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Non-blocking input phase: poll one event and route it (palette,
    /// autocomplete, quit, or normal action), rendering immediately for
    /// responsiveness. Returns how the main loop should proceed this tick.
    pub(super) fn handle_input_phase(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        current_ms: u64,
    ) -> io::Result<InputOutcome> {
        // Reuse the idle poll that ended the previous iteration; only poll
        // (crossterm reader lock + kevent syscall) when there is none — first
        // iteration, or after a `Restart` skipped the idle poll.
        let ready = self.input_ready.take().map_or_else(
            || {
                let _guard = crate::profile!("event_poll");
                event::poll(Duration::ZERO)
            },
            Ok,
        )?;
        if !ready {
            return Ok(InputOutcome::Continue);
        }
        let evt = {
            let _guard = crate::profile!("event_read");
            event::read()?
        };

        if self.route_modal_event(&evt) {
            self.render_frame(terminal, current_ms)?;
            return Ok(InputOutcome::Restart);
        }

        let mapped = {
            let _guard = crate::profile!("handle_event");
            handle_event(&evt, &self.state)
        };
        let Some(action) = mapped else {
            // User quit — flush all pending writes and save final state synchronously
            let _guard = crate::profile!("quit_flush");
            self.writer.flush();
            self.save_all_threads();
            return Ok(InputOutcome::Quit);
        };

        // Ignored event (mouse click/move, unbound key): nothing changed, so
        // don't mark dirty and don't redraw.
        if matches!(action, Action::None) {
            return Ok(InputOutcome::Continue);
        }
        if matches!(action, Action::OpenCommandPalette) {
            let _guard = crate::profile!("open_palette");
            self.command_palette.open(&self.state);
            self.state.flags.ui.dirty = true;
        } else {
            let _guard = crate::profile!("handle_action");
            self.handle_action(action);
        }

        // Make the resident follow a focus change the action may have just made
        // (e.g. Right-arrow drill-in switching the focused thread) BEFORE the
        // post-input render below — else this frame paints the previous resident
        // ("one stale frame until I type" bug). No-op when focus did not change.
        {
            let _guard = crate::profile!("relocate_resident");
            self.relocate_resident_on_focus_change();
        }

        // Render immediately after input for instant feedback, but never faster
        // than the frame cap; otherwise the frame stays dirty and the throttled
        // render step later in the loop paints it.
        if self.state.flags.ui.dirty && current_ms.saturating_sub(self.last_render_ms) >= RENDER_THROTTLE_MS {
            self.render_frame(terminal, current_ms)?;
        }
        Ok(InputOutcome::Continue)
    }

    /// Route `evt` to an open modal (command palette first, then autocomplete).
    /// `true` = consumed and marked dirty: the caller renders and restarts.
    fn route_modal_event(&mut self, evt: &event::Event) -> bool {
        if self.command_palette.is_open {
            let _guard = crate::profile!("palette_event");
            if let Some(action) = self.handle_palette_event(evt) {
                self.handle_action(action);
            }
            self.state.flags.ui.dirty = true;
            return true;
        }
        let autocomplete_active = {
            let _guard = crate::profile!("autocomplete_check");
            self.state.get_ext::<cp_base::state::autocomplete::Suggestions>().is_some_and(|ac| ac.active)
        };
        if !autocomplete_active {
            return false;
        }
        let _guard = crate::profile!("autocomplete_event");
        self.handle_autocomplete_event(evt);
        self.state.flags.ui.dirty = true;
        true
    }
}
