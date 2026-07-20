//! Context Pilot — AI-powered TUI coding assistant.
//!
//! Entry point: sets up the terminal, loads state, initializes modules,
//! and runs the main event loop. Also handles `typst-compile` and
//! `typst-recompile-watched` subcommands for callback scripts.

// Force vendored OpenSSL for cross-compilation (activates openssl-sys/vendored).
// The tui crate doesn't call openssl directly — this is purely for feature unification.
use openssl as _;

/// Application logic: event loop, actions, context preparation.
mod app;
/// Infrastructure: API clients, tools, constants, file watchers.
mod infra;
/// LLM provider abstraction and streaming.
mod llms;
/// Module system: panels, tools, and context providers.
mod modules;
/// Persistent and runtime state management.
mod state;
/// Terminal UI: rendering, input, theme, sidebar.
mod ui;

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::mpsc;

use ratatui::prelude::{Constraint, CrosstermBackend, Direction, Layout, Line, Modifier, Rect, Span, Style, Terminal};

use infra::constants::theme;

// ─── Boot Screen ────────────────────────────────────────────────────────────
// Phased loading with visual progress — no more black void on startup.

/// Index constants for boot steps — avoids raw integer indexing.
const STEP_CONFIG: usize = 0;
/// Boot step index: loading panels.
const STEP_PANELS: usize = 1;
/// Boot step index: loading messages.
const STEP_MESSAGES: usize = 2;
/// Boot step index: assembling state.
const STEP_ASSEMBLE: usize = 3;
/// Boot step index: initializing modules.
const STEP_MODULES: usize = 4;
/// Boot step index: preparing workspace.
const STEP_WORKSPACE: usize = 5;
/// Total number of boot steps.
const BOOT_STEP_COUNT: usize = 6;

/// A single boot step shown in the loading screen.
struct BootStep {
    /// Human-readable label for this step.
    label: &'static str,
    /// Optional detail string shown in parentheses after the label.
    detail: Option<String>,
    /// Whether this step has completed.
    done: bool,
}

/// Render the boot screen with completed/in-progress steps and a progress bar.
fn render_boot_screen(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, steps: &[BootStep]) {
    let done_count = steps.iter().filter(|s| s.done).count();
    let total = steps.len().max(1); // avoid division by zero

    drop(terminal.draw(|frame| {
        let area = frame.area();

        // Centered box: 50 wide, 2 (title) + steps + 2 (gauge + padding)
        let raw_height = steps.len().saturating_add(5).min(usize::from(area.height));
        let box_height = u16::try_from(raw_height).unwrap_or(area.height);
        let box_width = 50.min(area.width);
        // center horizontally: (width - box_width) / 2
        let x = {
            let diff = area.width.saturating_sub(box_width);
            diff >> 1i32 // equivalent to / 2 without triggering the lint
        };
        let y = {
            let diff = area.height.saturating_sub(box_height);
            diff >> 1i32
        };
        let boot_area = Rect::new(x, y, box_width, box_height);

        // Split: steps area + gauge
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // blank
                Constraint::Min(1),    // steps
                Constraint::Length(1), // blank
                Constraint::Length(1), // gauge
            ])
            .split(boot_area);
        debug_assert!(chunks.len() >= BOOT_STEP_COUNT.saturating_sub(1), "layout must produce at least 5 chunks");

        let Some(title_area) = chunks.first().copied() else { return };
        let Some(steps_area) = chunks.get(2).copied() else { return };
        let Some(gauge_area) = chunks.get(4).copied() else { return };

        // Title
        let title = Line::from(vec![
            Span::styled("\u{2693} ", Style::default().fg(theme::accent())),
            Span::styled("Context Pilot", Style::default().fg(theme::text()).add_modifier(Modifier::BOLD)),
        ]);
        frame.render_widget(title, title_area);

        // Steps
        let step_lines: Vec<Line<'_>> = steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let (icon, style) = if step.done {
                    ("  \u{2713} ", Style::default().fg(theme::success()))
                } else if i == done_count {
                    ("  \u{25b8} ", Style::default().fg(theme::warning()))
                } else {
                    ("    ", Style::default().fg(theme::text_muted()))
                };
                let detail = step.detail.as_deref().unwrap_or("");
                let text = if detail.is_empty() {
                    format!("{icon}{}", step.label)
                } else {
                    format!("{icon}{} ({detail})", step.label)
                };
                Line::from(Span::styled(text, style))
            })
            .collect();
        let steps_widget = ratatui::widgets::Paragraph::new(step_lines);
        frame.render_widget(steps_widget, steps_area);

        // Progress gauge — pure integer arithmetic to avoid float cast lints
        let pct = done_count.saturating_mul(100).checked_div(total).unwrap_or(0);
        let gauge_width = gauge_area.width;
        let filled_usize = done_count.saturating_mul(usize::from(gauge_width)).checked_div(total).unwrap_or(0);
        let filled = u16::try_from(filled_usize).unwrap_or(gauge_width);
        let mut gauge_bar = "\u{2588}".repeat(filled_usize);
        gauge_bar.push_str(&"\u{2591}".repeat(usize::from(gauge_width.saturating_sub(filled))));
        let gauge_line = Line::from(vec![
            Span::styled(gauge_bar, Style::default().fg(theme::accent())),
            Span::raw(format!(" {pct}%")),
        ]);
        frame.render_widget(gauge_line, gauge_area);
    }));
}

// ─── File Logger ────────────────────────────────────────────────────────────
// Minimal `log` backend that appends trace-level messages to a single file.
// Registered once at startup; no-ops if the file can't be opened.

/// File-backed logger that writes trace-level messages to `.context-pilot/state-machine.log`.
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

/// Raise the process file-descriptor soft limit to avoid "too many open files"
/// errors. The Meilisearch server, file watchers, indexer, and console server
/// collectively need hundreds of FDs. macOS defaults to a soft limit of 256,
/// which is too low. We raise it to `min(hard_limit, 8192)` — no root needed.
fn raise_fd_limit() {
    let Ok((soft, hard)) = rlimit::getrlimit(rlimit::Resource::NOFILE) else {
        return;
    };
    let target = hard.min(8192);
    if soft < target {
        let _r = rlimit::setrlimit(rlimit::Resource::NOFILE, target, hard);
    }
}

use crossterm::{
    ExecutableCommand as _,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use app::{App, ensure_default_agent, ensure_default_contexts};
use infra::api::StreamEvent;
use state::cache::CacheUpdate;
use state::persistence::{
    boot_assemble_state, boot_extract_module_data, boot_init_modules, boot_load_config, boot_load_messages,
    boot_load_panels, load_state,
};

fn main() -> ExitCode {
    /// Helper to mark a boot step as done, with bounds checking.
    fn mark_step_done(steps: &mut [BootStep], idx: usize) {
        if let Some(step) = steps.get_mut(idx) {
            step.done = true;
        }
    }

    /// Helper to set a boot step's detail, with bounds checking.
    fn set_step_detail(steps: &mut [BootStep], idx: usize, detail: String) {
        if let Some(step) = steps.get_mut(idx) {
            step.detail = Some(detail);
        }
    }

    init_file_logger();
    raise_fd_limit();
    infra::flame::init();

    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let resume_stream = args.iter().any(|a| a == "--resume-stream");

    // --bridge: activate the orchestration bridge (equivalent to CP_BRIDGE=1).
    // Uses a safe OnceLock flag so BridgeModule::init_state picks it up during boot.
    if args.iter().any(|a| a == "--bridge") {
        cp_mod_bridge::request_bridge();
    }

    // Panic hook: restore terminal state and log the panic to disk.
    // Without this, a panic leaves the terminal in raw mode + alternate screen,
    // which corrupts the SSH session and the error is lost.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _r_raw = disable_raw_mode();
        let _r_paste = io::stdout().execute(DisableBracketedPaste);
        let _r_screen = io::stdout().execute(LeaveAlternateScreen);

        // Write panic info to .context-pilot/errors/panic.log
        let error_dir = std::path::Path::new(".context-pilot").join("errors");
        let _r_mkdir = std::fs::create_dir_all(&error_dir);
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let backtrace = std::backtrace::Backtrace::force_capture();
        let msg = format!("[{ts}] {info}\n\n{backtrace}\n\n---\n");
        let log_path = error_dir.join("panic.log");
        let _r_write = std::fs::OpenOptions::new()
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
    let _r_enter = io::stdout().execute(EnterAlternateScreen);
    let _r_paste_on = io::stdout().execute(EnableBracketedPaste);
    let Ok(mut terminal) = Terminal::new(CrosstermBackend::new(io::stdout())) else {
        let _r_cleanup = disable_raw_mode();
        drop(writeln!(io::stderr(), "Fatal: failed to create terminal"));
        return ExitCode::FAILURE;
    };

    // ─── Phased boot with progress rendering ────────────────────────────
    let mut steps = vec![
        BootStep { label: "Loading config", detail: None, done: false },
        BootStep { label: "Loading panels", detail: None, done: false },
        BootStep { label: "Loading messages", detail: None, done: false },
        BootStep { label: "Assembling state", detail: None, done: false },
        BootStep { label: "Initializing modules", detail: None, done: false },
        BootStep { label: "Preparing workspace", detail: None, done: false },
    ];

    // Show initial boot screen immediately — banish the black void
    render_boot_screen(&mut terminal, &steps);

    // Detect new vs fresh-start format
    let new_format = std::path::Path::new(".context-pilot").join("config.json").exists();

    let mut state = if new_format {
        // Phase 1: Load config + worker state
        let cfg = boot_load_config();
        let module_data = boot_extract_module_data(&cfg);
        mark_step_done(&mut steps, STEP_CONFIG);
        render_boot_screen(&mut terminal, &steps);

        // Phase 2: Build context from panel JSONs
        let panels = boot_load_panels(&cfg);
        set_step_detail(&mut steps, STEP_PANELS, format!("{} panels", panels.panel_count));
        mark_step_done(&mut steps, STEP_PANELS);
        render_boot_screen(&mut terminal, &steps);

        // Phase 3: Load conversation messages from YAML
        let msg_count = panels.message_uids.len();
        let messages = boot_load_messages(&panels.message_uids);
        set_step_detail(&mut steps, STEP_MESSAGES, format!("{msg_count} messages"));
        mark_step_done(&mut steps, STEP_MESSAGES);
        render_boot_screen(&mut terminal, &steps);

        // Phase 4: Assemble state (without module init)
        let mut assembled_state = boot_assemble_state(cfg, panels, messages);
        mark_step_done(&mut steps, STEP_ASSEMBLE);
        render_boot_screen(&mut terminal, &steps);

        // Phase 5: Initialize modules (with per-module progress)
        boot_init_modules(&mut assembled_state, &module_data, |module_name| {
            set_step_detail(&mut steps, STEP_MODULES, module_name.to_owned());
            render_boot_screen(&mut terminal, &steps);
        });
        mark_step_done(&mut steps, STEP_MODULES);
        set_step_detail(&mut steps, STEP_WORKSPACE, "registering types".to_owned());
        render_boot_screen(&mut terminal, &steps);

        assembled_state
    } else {
        // Fresh start — no files to load, just create default state
        let s = load_state();
        mark_step_done(&mut steps, STEP_CONFIG);
        mark_step_done(&mut steps, STEP_PANELS);
        mark_step_done(&mut steps, STEP_MESSAGES);
        mark_step_done(&mut steps, STEP_ASSEMBLE);
        mark_step_done(&mut steps, STEP_MODULES);
        render_boot_screen(&mut terminal, &steps);
        s
    };

    // Phase 4 continued: Initialize modules
    state.highlight_ir_fn = Some(ui::helpers::highlight_file_ir);
    modules::validate_dependencies(&state.active_modules);
    modules::init_registry();

    // Remove orphaned context elements whose module no longer exists
    {
        let known_types: std::collections::HashSet<String> = modules::all_modules()
            .iter()
            .flat_map(|m| {
                let mut types: Vec<String> =
                    m.dynamic_panel_types().into_iter().map(|ct| ct.as_str().to_owned()).collect();
                types.extend(m.fixed_panel_types().into_iter().map(|ct| ct.as_str().to_owned()));
                types.extend(m.context_type_metadata().into_iter().map(|meta| meta.context_type.to_owned()));
                types
            })
            .collect();
        state.context.retain(|c| known_types.contains(c.context_type.as_str()));
    }

    // Phase 6: Prepare workspace
    ensure_default_contexts(&mut state);
    ensure_default_agent(&mut state);
    mark_step_done(&mut steps, STEP_WORKSPACE);
    render_boot_screen(&mut terminal, &steps);

    // Create channels
    let (tx, rx) = mpsc::channel::<StreamEvent>();
    let (cache_tx, cache_rx) = mpsc::channel::<CacheUpdate>();

    // Create and run app
    let mut app = App::new(state, cache_tx, resume_stream);
    let ch = app::run::lifecycle::EventChannels { tx: &tx, rx: &rx, cache_rx: &cache_rx };
    let run_result = app.run(&mut terminal, &ch);

    // Cleanup
    let _r_raw_off = disable_raw_mode();
    let _r_paste_off = io::stdout().execute(DisableBracketedPaste);
    let _r_leave = io::stdout().execute(LeaveAlternateScreen);
    infra::flame::flush();

    // Self-restart on reload — lets `cpilot` work without the run.sh supervisor loop.
    // exec() replaces this process with a fresh instance (same binary, same env).
    // Skipped when run.sh supervises (CP_RUN_SH=1) — the supervisor rebuilds via cargo run.
    // If exec fails, fall through to normal exit (run.sh catches it as before).
    #[cfg(unix)]
    if app.state.flags.lifecycle.reload_pending
        && std::env::var_os("CP_RUN_SH").is_none()
        && let Ok(exe_path) = std::env::current_exe()
    {
        use std::os::unix::process::CommandExt as _;
        let mut exec_args: Vec<String> = std::env::args().skip(1).collect();
        if !exec_args.iter().any(|a| a == "--resume-stream") {
            exec_args.push("--resume-stream".to_owned());
        }
        // Replaces the current process — never returns on success
        let _err = std::process::Command::new(exe_path).args(&exec_args).exec();
    }

    if let Err(e) = run_result {
        drop(writeln!(io::stderr(), "Fatal: {e}"));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
