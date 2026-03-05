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

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::mpsc;

// ─── File Logger ────────────────────────────────────────────────────────────
// Minimal `log` backend that appends trace-level messages to a single file.
// Registered once at startup; no-ops if the file can't be opened.

struct FileLogger(Mutex<Option<std::fs::File>>);

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        // Only accept our own state-machine traces — ignore noise from mio, polling, inotify, etc.
        metadata.level() <= log::Level::Trace && metadata.target().starts_with("cp_base")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata())
            && let Ok(mut guard) = self.0.lock()
            && let Some(f) = guard.as_mut()
        {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            drop(writeln!(f, "[{ts}] {} — {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.0.lock()
            && let Some(f) = guard.as_mut()
        {
            drop(Write::flush(f));
        }
    }
}

/// Best-effort logger init: writes to `.context-pilot/state-machine.log`.
/// Silently no-ops if the file or logger registration fails.
fn init_file_logger() {
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(".context-pilot/state-machine.log")
    else {
        return;
    };
    let logger = Box::leak(Box::new(FileLogger(Mutex::new(Some(file)))));
    drop(log::set_logger(logger));
    log::set_max_level(log::LevelFilter::Trace);
}

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

fn main() -> ExitCode {
    init_file_logger();

    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let resume_stream = args.iter().any(|a| a == "--resume-stream");

    // Handle typst subcommands (used by callback scripts)
    if args.len() >= 2 {
        match args[1].as_str() {
            // Compile a .typ → .pdf in the same directory
            "typst-compile" => return handle_cli_result(typst_cli::run_typst_compile(&args[2..])),
            // Recompile watched documents whose dependencies changed
            "typst-recompile-watched" => {
                return handle_cli_result(typst_cli::run_typst_recompile_watched(&args[2..]));
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
        let _r = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .and_then(|mut f| f.write_all(msg.as_bytes()));

        default_hook(info);
    }));

    let Ok(()) = enable_raw_mode() else {
        drop(writeln!(io::stderr(), "Fatal: failed to enable raw mode"));
        return ExitCode::FAILURE;
    };
    let _r = io::stdout().execute(EnterAlternateScreen);
    let _r = io::stdout().execute(EnableBracketedPaste);
    let Ok(mut terminal) = Terminal::new(CrosstermBackend::new(io::stdout())) else {
        let _r = disable_raw_mode();
        drop(writeln!(io::stderr(), "Fatal: failed to create terminal"));
        return ExitCode::FAILURE;
    };

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
    let run_result = app.run(&mut terminal, &tx, &rx, &cache_rx);

    // Cleanup
    let _r = disable_raw_mode();
    let _r = io::stdout().execute(DisableBracketedPaste);
    let _r = io::stdout().execute(LeaveAlternateScreen);

    if let Err(e) = run_result {
        drop(writeln!(io::stderr(), "Fatal: {e}"));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Handle a CLI subcommand result: write output and return an exit code.
///
/// `Ok(msg)` writes to stdout (if non-empty) and returns `SUCCESS`.
/// `Err((msg, code))` writes to stderr (if non-empty) and returns `FAILURE`
/// (or `SUCCESS` for exit code 0).
fn handle_cli_result(result: Result<String, (String, i32)>) -> ExitCode {
    match result {
        Ok(msg) => {
            if !msg.is_empty() {
                drop(writeln!(io::stdout(), "{msg}"));
            }
            ExitCode::SUCCESS
        }
        Err((msg, code)) => {
            if !msg.is_empty() {
                drop(writeln!(io::stderr(), "{msg}"));
            }
            ExitCode::from(u8::try_from(code.clamp(0, 255)).unwrap_or(1))
        }
    }
}
