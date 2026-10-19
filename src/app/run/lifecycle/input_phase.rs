//! The main loop's input phase (`loop.input`), split from `lifecycle/mod.rs`.
//!
//! Every statement that costs time sits under a named profile guard, so the
//! `loop.input.*` children sum to `loop.input` with no uncovered remainder.
//! Only bool tests and early returns run outside a guard.

use std::io;
use std::time::Duration;

use crossterm::event;

use crate::app::App;
use crate::app::actions::Action;
use crate::app::events::handle_event;
use crate::infra::constants::RENDER_THROTTLE_MS;

use super::InputOutcome;

/// Most events routed in one input phase before rendering: bounds the phase
/// when input floods (held key, wheel burst) so background work still runs.
const MAX_EVENTS_PER_PHASE: usize = 64;

/// What routing one event did.
enum EventStep {
    /// A modal (palette, autocomplete) consumed it.
    Modal,
    /// Routed normally (or ignored).
    Handled,
    /// User quit — state already flushed and saved.
    Quit,
}

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Non-blocking input phase: route every pending event (up to
    /// [`MAX_EVENTS_PER_PHASE`]), then render once. A burst of N events costs
    /// one frame instead of N. Returns how the main loop should proceed.
    pub(super) fn handle_input_phase(
        &mut self,
        terminal: &mut crate::ui::perf::tty::Tty,
        current_ms: u64,
    ) -> io::Result<InputOutcome> {
        // Reuse the idle poll that ended the previous iteration; only poll
        // (crossterm reader lock + kevent syscall) when there is none — first
        // iteration, or after a `Restart` skipped the idle poll.
        let mut ready = self.input_ready.take().map_or_else(poll_now, Ok)?;
        if !ready {
            return Ok(InputOutcome::Continue);
        }
        let mut modal = false;
        let mut routed = 0usize;
        while ready && routed < MAX_EVENTS_PER_PHASE {
            let evt = {
                let _guard = crate::profile!("event_read");
                event::read()?
            };
            match self.route_event(&evt) {
                EventStep::Quit => return Ok(InputOutcome::Quit),
                EventStep::Modal => modal = true,
                EventStep::Handled => {}
            }
            routed = routed.saturating_add(1);
            ready = {
                let _guard = crate::profile!("event_poll");
                poll_now()?
            };
        }
        if modal {
            self.render_frame(terminal, current_ms)?;
            return Ok(InputOutcome::Restart);
        }
        // Render immediately after input for instant feedback, but never faster
        // than the frame cap; otherwise the frame stays dirty and the throttled
        // render step later in the loop paints it.
        if self.state.flags.ui.dirty && current_ms.saturating_sub(self.last_render_ms) >= RENDER_THROTTLE_MS {
            self.render_frame(terminal, current_ms)?;
        }
        Ok(InputOutcome::Continue)
    }

    /// Route one event: resize bookkeeping, modals, quit, or a normal action.
    fn route_event(&mut self, evt: &event::Event) -> EventStep {
        if matches!(*evt, event::Event::Resize(..)) {
            // Next loop-tail render repaints every cell: resize leaves
            // artifacts the cell diff cannot see.
            self.last_full_redraw_ms = 0;
            self.state.flags.ui.dirty = true;
        }
        let modal = {
            let _guard = crate::profile!("route_modal");
            self.route_modal_event(evt)
        };
        if modal {
            return EventStep::Modal;
        }
        let mapped = {
            let _guard = crate::profile!("handle_event");
            handle_event(evt, &self.state)
        };
        let Some(action) = mapped else {
            // User quit — flush all pending writes and save final state synchronously
            let _guard = crate::profile!("quit_flush");
            self.writer.flush();
            self.save_all_agents();
            return EventStep::Quit;
        };
        // Ignored event (mouse click/move, unbound key): nothing changed, so
        // don't mark dirty and don't redraw.
        if matches!(action, Action::None) {
            return EventStep::Handled;
        }
        let _guard = crate::profile!("apply_input_action");
        self.apply_input_action(action);
        EventStep::Handled
    }

    /// Apply a mapped input action, then make the executing agent follow a
    /// focus change it may have made.
    fn apply_input_action(&mut self, action: Action) {
        if matches!(action, Action::OpenCommandPalette) {
            let _guard = crate::profile!("open_palette");
            self.command_palette.open(&self.state);
            self.state.flags.ui.dirty = true;
        } else {
            let _guard = crate::profile!("handle_action");
            self.handle_action(action);
        }
        // Make the executing follow a focus change the action may have just made
        // (e.g. Right-arrow drill-in switching the focused agent) BEFORE the
        // post-input render — else that frame paints the previous executing
        // ("one stale frame until I type" bug). No-op when focus did not change.
        let _guard = crate::profile!("follow_focus");
        self.follow_focus();
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

/// Non-blocking check for a pending input event.
fn poll_now() -> io::Result<bool> {
    let _guard = crate::profile!("event_poll");
    event::poll(Duration::ZERO)
}
