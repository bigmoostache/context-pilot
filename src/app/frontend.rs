//! Frontend abstraction — the seam that makes the core headless-agnostic.
//!
//! The event loop is generic over two traits:
//! - [`InputSource`] — where actions come from (crossterm events, web commands);
//! - [`OutputSink`] — where state is presented (ratatui terminal, WebSocket broadcast).
//!
//! The TUI and the web server are two implementations of the same traits;
//! keeping both guarantees the core never grows terminal-only assumptions.

use std::io;
use std::time::Duration;

use crossterm::event;
use ratatui::prelude::{CrosstermBackend, Terminal};

use crate::app::App;
use crate::app::actions::Action;
use crate::app::events::handle_event;
use crate::app::run::lifecycle::EventChannels;
use crate::ui;

/// Flow-control result of pumping an input source once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PumpFlow {
    /// No input was available.
    Idle,
    /// At least one input was handled — render soon for responsive feedback.
    Handled,
    /// The user asked to quit (Ctrl+Q on the TUI). Headless sources never quit.
    Quit,
}

/// A source of user input feeding the event loop.
///
/// Implementations translate their native input (key events, WebSocket
/// commands) into [`Action`]s and direct state mutations, exactly like the
/// historical crossterm path did.
pub(crate) trait InputSource {
    /// Drain at most one pending input item, dispatching it into `app`.
    fn pump(&mut self, app: &mut App, ch: &EventChannels<'_>) -> io::Result<PumpFlow>;

    /// Block up to `ms` milliseconds waiting for new input (idle sleep).
    /// Only the loop's primary source is asked to wait.
    fn wait(&mut self, ms: u64);
}

/// A presentation target for the application state.
pub(crate) trait OutputSink {
    /// Present the current state. Called by the loop when the state is dirty
    /// (immediately after input, throttled otherwise). Implementations must
    /// not clear `flags.ui.dirty` — the loop owns that flag.
    fn present(&mut self, app: &mut App) -> io::Result<()>;
}

// ─── TUI implementations ────────────────────────────────────────────────────

/// Crossterm-backed input source (raw terminal events).
///
/// Unit struct: crossterm reads events through global functions, so no
/// terminal handle is needed — which is what lets input and output split
/// cleanly into two traits.
pub(crate) struct TuiInput;

impl InputSource for TuiInput {
    fn pump(&mut self, app: &mut App, ch: &EventChannels<'_>) -> io::Result<PumpFlow> {
        if !event::poll(Duration::ZERO)? {
            return Ok(PumpFlow::Idle);
        }
        let evt = event::read()?;

        // Command palette consumes events while open
        if app.command_palette.is_open {
            if let Some(action) = app.handle_palette_event(&evt) {
                app.handle_action(action, ch.tx);
            }
            app.state.flags.ui.dirty = true;
            return Ok(PumpFlow::Handled);
        }

        // @-autocomplete popup consumes events while active
        if let Some(ac) = app.state.get_ext::<cp_base::state::autocomplete::Suggestions>()
            && ac.active
        {
            app.handle_autocomplete_event(&evt);
            app.state.flags.ui.dirty = true;
            return Ok(PumpFlow::Handled);
        }

        // Question form consumes events while unresolved (mutates state directly)
        if let Some(form) = app.state.get_ext::<cp_base::ui::question_form::PendingForm>()
            && !form.resolved
        {
            app.handle_question_form_event(&evt);
            app.state.flags.ui.dirty = true;
            return Ok(PumpFlow::Handled);
        }

        let Some(action) = handle_event(&evt, &app.state) else {
            return Ok(PumpFlow::Quit);
        };

        // Ctrl+P opens the palette (TUI-owned overlay)
        if matches!(action, Action::OpenCommandPalette) {
            app.command_palette.open(&app.state);
            app.state.flags.ui.dirty = true;
        } else {
            app.handle_action(action, ch.tx);
        }
        Ok(PumpFlow::Handled)
    }

    fn wait(&mut self, ms: u64) {
        // Doubles as the loop's idle sleep — wakes early on any terminal event.
        let _r = event::poll(Duration::from_millis(ms));
    }
}

/// Ratatui-backed output sink (owns the terminal).
pub(crate) struct TuiOutput {
    /// The ratatui terminal this sink draws to.
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TuiOutput {
    /// Wrap a ratatui terminal as an [`OutputSink`].
    pub(crate) const fn new(terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Self {
        Self { terminal }
    }
}

impl OutputSink for TuiOutput {
    fn present(&mut self, app: &mut App) -> io::Result<()> {
        let _r = self.terminal.draw(|frame| {
            ui::render(frame, &mut app.state);
            app.command_palette.render(frame, &app.state);
        })?;
        Ok(())
    }
}
