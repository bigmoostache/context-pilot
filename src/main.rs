//! Context Pilot — AI-powered TUI coding assistant.
//!
//! Entry point: sets up the terminal, loads state, initializes modules,
//! and runs the main event loop. Also handles `typst-compile` and
//! `typst-recompile-watched` subcommands for callback scripts.

mod app;
mod infra;
mod llms;
mod modules;
mod state;
mod typst_cli;
mod ui;

use std::io;
use std::sync::mpsc;

use crossterm::{
    ExecutableCommand,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;

use app::{App, ensure_default_agent, ensure_default_contexts};
use infra::api::StreamEvent;
use state::cache::CacheUpdate;
use state::persistence::load_state;

fn main() -> io::Result<()> {
    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let resume_stream = args.iter().any(|a| a == "--resume-stream");

    // Handle typst subcommands (used by callback scripts)
    if args.len() >= 2 {
        match args[1].as_str() {
            // Compile a .typ → .pdf in the same directory
            "typst-compile" => handle_cli_result(typst_cli::run_typst_compile(&args[2..])),
            // Recompile watched documents whose dependencies changed
            "typst-recompile-watched" => {
                handle_cli_result(typst_cli::run_typst_recompile_watched(&args[2..]));
            }
            _ => {}
        }
    }

    // Panic hook: restore terminal state and log the panic to disk.
    // Without this, a panic leaves the terminal in raw mode + alternate screen,
    // which corrupts the SSH session and the error is lost.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _r = disable_raw_mode();
        let _r = io::stdout().execute(DisableBracketedPaste);
        let _r = io::stdout().execute(LeaveAlternateScreen);

        // Write panic info to .context-pilot/errors/panic.log
        let error_dir = std::path::Path::new(".context-pilot").join("errors");
        let _r = std::fs::create_dir_all(&error_dir);
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let backtrace = std::backtrace::Backtrace::force_capture();
        let msg = format!("[{ts}] {info}\n\n{backtrace}\n\n---\n");
        let log_path = error_dir.join("panic.log");
        let _r = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).and_then(|mut f| {
            use std::io::Write;
            f.write_all(msg.as_bytes())
        });

        default_hook(info);
    }));

    enable_raw_mode()?;
    let _r = io::stdout().execute(EnterAlternateScreen)?;
    let _r = io::stdout().execute(EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut state = load_state();

    // Set callback hooks for extracted module crates
    state.highlight_fn = Some(ui::helpers::highlight_file);

    // Validate module dependencies at startup
    modules::validate_dependencies(&state.active_modules);

    // Initialize the ContextType registry from all modules (must happen before any is_fixed/icon/needs_cache calls)
    modules::init_registry();

    // Remove orphaned context elements whose module no longer exists
    // (e.g., tmux panels persisted before the tmux crate was removed).
    {
        let known_types: std::collections::HashSet<String> = modules::all_modules()
            .iter()
            .flat_map(|m| {
                let mut types: Vec<String> =
                    m.dynamic_panel_types().into_iter().map(|ct| ct.as_str().to_string()).collect();
                types.extend(m.fixed_panel_types().into_iter().map(|ct| ct.as_str().to_string()));
                types.extend(m.context_type_metadata().into_iter().map(|meta| meta.context_type.to_string()));
                types
            })
            .collect();
        state.context.retain(|c| known_types.contains(c.context_type.as_str()));
    }

    // Ensure default context elements and seed exist
    ensure_default_contexts(&mut state);
    ensure_default_agent(&mut state);

    // Ensure built-in presets exist on disk
    cp_mod_preset::builtin::ensure_builtin_presets();

    // Create channels
    let (tx, rx) = mpsc::channel::<StreamEvent>();
    let (cache_tx, cache_rx) = mpsc::channel::<CacheUpdate>();

    // Create and run app
    let mut app = App::new(state, cache_tx, resume_stream);
    app.run(&mut terminal, &tx, &rx, &cache_rx)?;

    // Cleanup
    disable_raw_mode()?;
    let _r = io::stdout().execute(DisableBracketedPaste)?;
    let _r = io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

/// Handle a CLI subcommand result: print output and exit.
///
/// `Ok(msg)` prints to stdout (if non-empty) and exits 0.
/// `Err((msg, code))` prints to stderr (if non-empty) and exits with `code`.
#[expect(
    clippy::exit,
    clippy::print_stdout,
    reason = "CLI entry point — printing and process::exit are the correct interface"
)]
fn handle_cli_result(result: Result<String, (String, i32)>) -> ! {
    match result {
        Ok(msg) => {
            if !msg.is_empty() {
                println!("{msg}");
            }
            std::process::exit(0);
        }
        Err((msg, code)) => {
            if !msg.is_empty() {
                use io::Write;
                drop(writeln!(io::stderr(), "{msg}"));
            }
            std::process::exit(code);
        }
    }
}
