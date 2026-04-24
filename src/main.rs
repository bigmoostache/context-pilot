//! Context Pilot — AI-powered TUI coding assistant.
//!
//! Entry point: sets up the terminal, loads state, initializes modules,
//! and runs the main event loop. Also handles `typst-compile` and
//! `typst-recompile-watched` subcommands for callback scripts.

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
/// CLI subcommands for Typst compilation.
mod typst_cli;
/// Terminal UI: rendering, input, theme, sidebar.
mod ui;

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::mpsc;

use ratatui::prelude::{
    Color, Constraint, CrosstermBackend, Direction, Layout, Line, Modifier, Rect, Span, Style, Terminal,
};

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
        let raw_height = steps.len().saturating_add(5).min(area.height as usize);
        let box_height = u16::try_from(raw_height).unwrap_or(area.height);
        let box_width = 50.min(area.width);
        // center horizontally: (width - box_width) / 2
        let x = {
            let diff = area.width.saturating_sub(box_width);
            diff >> 1 // equivalent to / 2 without triggering the lint
        };
        let y = {
            let diff = area.height.saturating_sub(box_height);
            diff >> 1
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
            Span::styled("⚓ ", Style::default().fg(Color::Cyan)),
            Span::styled("Context Pilot", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]);
        frame.render_widget(title, title_area);

        // Steps
        let step_lines: Vec<Line<'_>> = steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let (icon, style) = if step.done {
                    ("  ✓ ", Style::default().fg(Color::Green))
                } else if i == done_count {
                    ("  ▸ ", Style::default().fg(Color::Yellow))
                } else {
                    ("    ", Style::default().fg(Color::DarkGray))
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
        let mut gauge_bar = "█".repeat(filled_usize);
        gauge_bar.push_str(&"░".repeat(usize::from(gauge_width.saturating_sub(filled))));
        let gauge_line =
            Line::from(vec![Span::styled(gauge_bar, Style::default().fg(Color::Cyan)), Span::raw(format!(" {pct}%"))]);
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

    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let resume_stream = args.iter().any(|a| a == "--resume-stream");

    // Handle typst subcommands (used by callback scripts)
    if args.len() >= 2 {
        let Some(subcommand) = args.get(1) else {
            return ExitCode::FAILURE;
        };
        let rest = args.get(2..).unwrap_or_default();
        match subcommand.as_str() {
            // Compile a .typ → .pdf in the same directory
            "typst-compile" => return handle_cli_result(typst_cli::run_typst_compile(rest)),
            // Recompile watched documents whose dependencies changed
            "typst-recompile-watched" => {
                return handle_cli_result(typst_cli::run_typst_recompile_watched(rest));
            }
            _ => {}
        }
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
            set_step_detail(&mut steps, STEP_MODULES, module_name.to_string());
            render_boot_screen(&mut terminal, &steps);
        });
        mark_step_done(&mut steps, STEP_MODULES);
        set_step_detail(&mut steps, STEP_WORKSPACE, "registering types".to_string());
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
    state.highlight_fn = Some(ui::helpers::highlight_file);
    state.highlight_ir_fn = Some(ui::helpers::highlight_file_ir);
    modules::validate_dependencies(&state.active_modules);
    modules::init_registry();

    // Remove orphaned context elements whose module no longer exists
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

    // Phase 6: Prepare workspace
    ensure_default_contexts(&mut state);
    ensure_default_agent(&mut state);
    cp_mod_preset::builtin::ensure_builtin_presets();
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
