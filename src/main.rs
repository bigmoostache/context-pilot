mod actions;
mod api;
mod background;
mod cache;
mod config;
mod constants;
mod core;
mod events;
mod help;
mod highlight;
mod llms;
mod perf;
mod persistence;
mod profiler;
mod state;
mod tool_defs;
mod modules;
mod tools;
mod typewriter;
mod ui;
mod watcher;

use std::io;
use std::sync::mpsc;

use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::*;

use api::StreamEvent;
use background::TlDrResult;
use cache::CacheUpdate;
use core::{ensure_default_contexts, ensure_default_seed, App};
use persistence::load_state;

fn main() -> io::Result<()> {
    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let resume_stream = args.iter().any(|a| a == "--resume-stream");

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut state = load_state();

    // Validate module dependencies at startup
    modules::validate_dependencies(&state.active_modules);

    // Ensure default context elements and seed exist
    ensure_default_contexts(&mut state);
    ensure_default_seed(&mut state);

    // Ensure built-in presets exist on disk
    modules::preset::builtin::ensure_builtin_presets();

    // Create channels
    let (tx, rx) = mpsc::channel::<StreamEvent>();
    let (tldr_tx, tldr_rx) = mpsc::channel::<TlDrResult>();
    let (cache_tx, cache_rx) = mpsc::channel::<CacheUpdate>();

    // Create and run app
    let mut app = App::new(state, cache_tx, resume_stream);
    app.run(&mut terminal, tx, rx, tldr_tx, tldr_rx, cache_rx)?;

    // Cleanup
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
