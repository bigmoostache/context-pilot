mod actions;
mod api;
mod background;
mod constants;
mod context_cleaner;
mod core;
mod events;
mod highlight;
mod mouse;
mod persistence;
mod state;
mod tool_defs;
mod tools;
mod typewriter;
mod ui;

use std::io;
use std::sync::mpsc;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::*;

use api::StreamEvent;
use background::TlDrResult;
use core::{ensure_default_contexts, App};
use persistence::load_state;

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    io::stdout().execute(EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut state = load_state();

    // Ensure default context elements exist
    ensure_default_contexts(&mut state);

    // Create channels
    let (tx, rx) = mpsc::channel::<StreamEvent>();
    let (tldr_tx, tldr_rx) = mpsc::channel::<TlDrResult>();
    let (clean_tx, clean_rx) = mpsc::channel::<StreamEvent>();

    // Create and run app
    let mut app = App::new(state);
    app.run(&mut terminal, tx, rx, tldr_tx, tldr_rx, clean_tx, clean_rx)?;

    // Cleanup
    io::stdout().execute(DisableMouseCapture)?;
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
