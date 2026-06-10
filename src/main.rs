//! Context Pilot — AI-powered TUI coding assistant.
//!
//! Entry point: sets up the terminal, loads state, initializes modules,
//! and runs the main event loop.  Also handles `typst-compile` and
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
/// Web frontend (Nestor): WebSource/WebSink + WebState builders.
mod web;

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
            Span::styled("⚓ ", Style::default().fg(theme::accent())),
            Span::styled("Context Pilot", Style::default().fg(theme::text()).add_modifier(Modifier::BOLD)),
        ]);
        frame.render_widget(title, title_area);

        // Steps
        let step_lines: Vec<Line<'_>> = steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let (icon, style) = if step.done {
                    ("  ✓ ", Style::default().fg(theme::success()))
                } else if i == done_count {
                    ("  ▸ ", Style::default().fg(theme::warning()))
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
        let mut gauge_bar = "█".repeat(filled_usize);
        gauge_bar.push_str(&"░".repeat(usize::from(gauge_width.saturating_sub(filled))));
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

/// Parsed command-line arguments.
struct CliArgs {
    /// Auto-resume streaming after a reload.
    resume_stream: bool,
    /// Run without a terminal (Nestor mode) — requires `--web-bind`.
    headless: bool,
    /// Address for the web server (enables the web frontend in any mode).
    web_bind: Option<std::net::SocketAddr>,
    /// Allow binding `0.0.0.0` (explicitly opted in).
    web_bind_any: bool,
    /// Directory of the built SPA.
    web_dist: std::path::PathBuf,
    /// Projects root: enables the workspace system (picker + switch).
    projects_dir: Option<std::path::PathBuf>,
}

/// Parse command-line arguments (tiny by design — no clap dependency).
fn parse_cli() -> CliArgs {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cli = CliArgs {
        resume_stream: false,
        headless: false,
        web_bind: None,
        web_bind_any: false,
        web_dist: std::path::PathBuf::from("web-dist"),
        projects_dir: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--resume-stream" => cli.resume_stream = true,
            "--headless" => cli.headless = true,
            "--web-bind-any" => cli.web_bind_any = true,
            "--web-bind" => cli.web_bind = iter.next().and_then(|v| v.parse().ok()),
            "--web-dist" => {
                if let Some(dir) = iter.next() {
                    // Absolu : le cwd change à chaque bascule de projet.
                    let path = std::path::PathBuf::from(dir);
                    cli.web_dist = path.canonicalize().unwrap_or(path);
                }
            }
            "--projects-dir" => {
                // Absolu obligatoire : exec() conserve le cwd de l'ancien
                // projet, le chemin doit rester valable après bascule.
                cli.projects_dir = iter
                    .next()
                    .map(std::path::PathBuf::from)
                    .map(|dir| dir.canonicalize().unwrap_or(dir));
            }
            _ => {}
        }
    }
    cli
}

/// Enter the active project under the projects root: read the `.current`
/// pointer (fall back to the most recent project, else create `scratch`),
/// chdir into it, and record its name for the web `meta` section.
/// Returns `None` when the workspace cannot be entered.
fn enter_project(projects_dir: &std::path::Path) -> Option<String> {
    use cp_web_server::projects;
    if let Err(e) = std::fs::create_dir_all(projects_dir) {
        drop(writeln!(io::stderr(), "Fatal: cannot create projects dir: {e}"));
        return None;
    }
    let name = projects::read_current(projects_dir)
        .filter(|n| projects_dir.join(n).is_dir())
        .or_else(|| {
            // Pas de pointeur valable : dernier projet actif, sinon « scratch ».
            let fallback = projects::list(projects_dir)
                .into_iter()
                .next()
                .map_or_else(|| "scratch".to_string(), |p| p.name);
            std::fs::create_dir_all(projects_dir.join(&fallback)).ok()?;
            projects::write_current(projects_dir, &fallback).ok()?;
            Some(fallback)
        })?;
    let path = projects_dir.join(&name);
    if let Err(e) = std::env::set_current_dir(&path) {
        drop(writeln!(io::stderr(), "Fatal: cannot enter project '{name}': {e}"));
        return None;
    }
    web::set_project_name(&name);
    Some(name)
}

/// Install the panic hook: restore the terminal (TUI mode only) and append
/// the panic + backtrace to `.context-pilot/errors/panic.log`.
fn install_panic_hook(restore_terminal: bool) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if restore_terminal {
            let _r_raw = disable_raw_mode();
            let _r_paste = io::stdout().execute(DisableBracketedPaste);
            let _r_screen = io::stdout().execute(LeaveAlternateScreen);
        }
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
}

/// Progress callback invoked by [`boot_phased`] after every step change.
type BootProgressFn<'cb> = &'cb mut dyn FnMut(&[BootStep]);

/// Run the phased boot, invoking `on_step` after every progress change.
/// The callback renders the boot screen (TUI) or logs to stderr (headless).
fn boot_phased(on_step: BootProgressFn<'_>) -> state::State {
    let mut steps = vec![
        BootStep { label: "Loading config", detail: None, done: false },
        BootStep { label: "Loading panels", detail: None, done: false },
        BootStep { label: "Loading messages", detail: None, done: false },
        BootStep { label: "Assembling state", detail: None, done: false },
        BootStep { label: "Initializing modules", detail: None, done: false },
        BootStep { label: "Preparing workspace", detail: None, done: false },
    ];
    on_step(&steps);

    // Detect new vs fresh-start format
    let new_format = std::path::Path::new(".context-pilot").join("config.json").exists();

    let mut state = if new_format {
        // Phase 1: Load config + worker state
        let cfg = boot_load_config();
        let module_data = boot_extract_module_data(&cfg);
        mark_step_done(&mut steps, STEP_CONFIG);
        on_step(&steps);

        // Phase 2: Build context from panel JSONs
        let panels = boot_load_panels(&cfg);
        set_step_detail(&mut steps, STEP_PANELS, format!("{} panels", panels.panel_count));
        mark_step_done(&mut steps, STEP_PANELS);
        on_step(&steps);

        // Phase 3: Load conversation messages from YAML
        let msg_count = panels.message_uids.len();
        let messages = boot_load_messages(&panels.message_uids);
        set_step_detail(&mut steps, STEP_MESSAGES, format!("{msg_count} messages"));
        mark_step_done(&mut steps, STEP_MESSAGES);
        on_step(&steps);

        // Phase 4: Assemble state (without module init)
        let mut assembled_state = boot_assemble_state(cfg, panels, messages);
        mark_step_done(&mut steps, STEP_ASSEMBLE);
        on_step(&steps);

        // Phase 5: Initialize modules (with per-module progress)
        boot_init_modules(&mut assembled_state, &module_data, |module_name| {
            set_step_detail(&mut steps, STEP_MODULES, module_name.to_string());
            on_step(&steps);
        });
        mark_step_done(&mut steps, STEP_MODULES);
        set_step_detail(&mut steps, STEP_WORKSPACE, "registering types".to_string());
        on_step(&steps);

        assembled_state
    } else {
        // Fresh start — no files to load, just create default state
        let fresh = load_state();
        mark_step_done(&mut steps, STEP_CONFIG);
        mark_step_done(&mut steps, STEP_PANELS);
        mark_step_done(&mut steps, STEP_MESSAGES);
        mark_step_done(&mut steps, STEP_ASSEMBLE);
        mark_step_done(&mut steps, STEP_MODULES);
        on_step(&steps);
        fresh
    };

    finalize_state(&mut state);
    mark_step_done(&mut steps, STEP_WORKSPACE);
    on_step(&steps);
    state
}

/// Post-boot initialization shared by the TUI and headless paths.
fn finalize_state(state: &mut state::State) {
    state.highlight_ir_fn = Some(ui::helpers::highlight_file_ir);
    modules::validate_dependencies(&state.active_modules);
    modules::init_registry();

    // Remove orphaned context elements whose module no longer exists
    let known_types: std::collections::HashSet<String> = modules::all_modules()
        .iter()
        .flat_map(|m| {
            let mut types: Vec<String> = m.dynamic_panel_types().into_iter().map(|ct| ct.as_str().to_string()).collect();
            types.extend(m.fixed_panel_types().into_iter().map(|ct| ct.as_str().to_string()));
            types.extend(m.context_type_metadata().into_iter().map(|meta| meta.context_type.to_string()));
            types
        })
        .collect();
    state.context.retain(|c| known_types.contains(c.context_type.as_str()));

    ensure_default_contexts(state);
    ensure_default_agent(state);
}

/// Start the web server when `--web-bind` was given.
/// Returns the web frontend pair, or `None` (with a logged error) on failure.
fn start_web(cli: &CliArgs) -> Option<(web::WebSource, web::WebSink)> {
    let bind = cli.web_bind?;
    let (events_tx, events_rx) = mpsc::channel::<cp_web_server::protocol::WebEvent>();
    // Auth globale ($HOME/.context-pilot/) : le token doit survivre aux
    // bascules de projet — chaque workspace a son propre .context-pilot/.
    let auth_path = std::env::var_os("HOME").map_or_else(
        || std::path::Path::new(".context-pilot").join("web-auth.json"),
        |home| std::path::Path::new(&home).join(".context-pilot").join("web-auth.json"),
    );
    let config = cp_web_server::WebServerConfig {
        bind,
        allow_any_bind: cli.web_bind_any,
        dist_dir: cli.web_dist.clone(),
        auth_path,
        initial_password: std::env::var("CP_WEB_PASSWORD").ok(),
        projects_root: cli.projects_dir.clone(),
    };
    match cp_web_server::start(&config, events_tx) {
        Ok(handle) => {
            let sink_handle = handle.clone();
            Some((web::WebSource::new(events_rx, handle, cli.projects_dir.clone()), web::WebSink::new(sink_handle)))
        }
        Err(e) => {
            drop(writeln!(io::stderr(), "Web server error: {e}"));
            None
        }
    }
}

/// Self-restart on reload — lets `cpilot` work without the run.sh supervisor
/// loop. `exec()` replaces this process with a fresh instance (same binary,
/// same env). Skipped when run.sh supervises (`CP_RUN_SH=1`).
///
/// The arguments are rebuilt from the parsed CLI (not `std::env::args()`):
/// after a project switch the cwd changes, so any relative path from the
/// original invocation would resolve wrong — the parsed paths are absolute.
fn exec_restart_if_pending(app: &App, cli: &CliArgs) {
    #[cfg(unix)]
    if app.state.flags.lifecycle.reload_pending
        && std::env::var_os("CP_RUN_SH").is_none()
        && let Ok(exe_path) = std::env::current_exe()
    {
        use std::os::unix::process::CommandExt as _;
        // Une bascule de projet ouvre une autre session : ne pas reprendre
        // le stream — c'est réservé aux reloads en place (system_reload).
        let mut restart_args: Vec<String> =
            if app.switch_pending { Vec::new() } else { vec!["--resume-stream".to_string()] };
        if cli.headless {
            restart_args.push("--headless".to_string());
        }
        if let Some(bind) = cli.web_bind {
            restart_args.push("--web-bind".to_string());
            restart_args.push(bind.to_string());
        }
        if cli.web_bind_any {
            restart_args.push("--web-bind-any".to_string());
        }
        restart_args.push("--web-dist".to_string());
        restart_args.push(cli.web_dist.display().to_string());
        if let Some(projects_dir) = &cli.projects_dir {
            restart_args.push("--projects-dir".to_string());
            restart_args.push(projects_dir.display().to_string());
        }
        // Replaces the current process — never returns on success
        let _err = std::process::Command::new(exe_path).args(&restart_args).exec();
    }
}

/// Headless entry point (Nestor): no terminal, web frontend only.
fn headless_main(cli: &CliArgs) -> ExitCode {
    install_panic_hook(false);

    if cli.web_bind.is_none() {
        drop(writeln!(io::stderr(), "Fatal: --headless requires --web-bind <ip:port>"));
        return ExitCode::FAILURE;
    }

    // Workspace system: enter the active project before any state I/O.
    if let Some(projects_dir) = &cli.projects_dir {
        let Some(project) = enter_project(projects_dir) else {
            return ExitCode::FAILURE;
        };
        drop(writeln!(io::stderr(), "[boot] project: {project}"));
    }

    // Boot with stderr progress lines instead of the boot screen.
    let mut last_done = usize::MAX;
    let state = boot_phased(&mut |steps| {
        let done = steps.iter().filter(|step| step.done).count();
        if done != last_done {
            last_done = done;
            if let Some(current) = steps.iter().find(|step| !step.done) {
                drop(writeln!(io::stderr(), "[boot {}/{}] {}…", done, steps.len(), current.label));
            }
        }
    });
    drop(writeln!(io::stderr(), "[boot] ready — headless"));

    let Some((mut web_source, mut web_sink)) = start_web(cli) else {
        return ExitCode::FAILURE;
    };

    let (tx, rx) = mpsc::channel::<StreamEvent>();
    let (cache_tx, cache_rx) = mpsc::channel::<CacheUpdate>();
    let mut app = App::new(state, cache_tx, cli.resume_stream);
    let ch = app::run::lifecycle::EventChannels { tx: &tx, rx: &rx, cache_rx: &cache_rx };
    let run_result = app.run(&mut [&mut web_source], &mut [&mut web_sink], &ch);

    infra::flame::flush();
    exec_restart_if_pending(&app, cli);

    if let Err(e) = run_result {
        drop(writeln!(io::stderr(), "Fatal: {e}"));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// TUI entry point — optionally with the web frontend alongside
/// (same session, same loop: the browser and the terminal coexist).
fn tui_main(cli: &CliArgs) -> ExitCode {
    install_panic_hook(true);

    // Workspace system (optionnel en TUI : nestor-tui peut aussi cd lui-même).
    if let Some(projects_dir) = &cli.projects_dir
        && enter_project(projects_dir).is_none()
    {
        return ExitCode::FAILURE;
    }

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

    let state = boot_phased(&mut |steps| render_boot_screen(&mut terminal, steps));

    let (tx, rx) = mpsc::channel::<StreamEvent>();
    let (cache_tx, cache_rx) = mpsc::channel::<CacheUpdate>();

    // The TUI is one InputSource/OutputSink pair; the web frontend (when
    // enabled) is another pair over the same generic loop.
    let mut app = App::new(state, cache_tx, cli.resume_stream);
    let ch = app::run::lifecycle::EventChannels { tx: &tx, rx: &rx, cache_rx: &cache_rx };
    let mut tui_input = app::frontend::TuiInput;
    let mut tui_output = app::frontend::TuiOutput::new(terminal);
    let web_pair = start_web(cli);

    let run_result = if let Some((mut web_source, mut web_sink)) = web_pair {
        app.run(&mut [&mut tui_input, &mut web_source], &mut [&mut tui_output, &mut web_sink], &ch)
    } else {
        app.run(&mut [&mut tui_input], &mut [&mut tui_output], &ch)
    };

    // Cleanup
    let _r_raw_off = disable_raw_mode();
    let _r_paste_off = io::stdout().execute(DisableBracketedPaste);
    let _r_leave = io::stdout().execute(LeaveAlternateScreen);
    infra::flame::flush();

    exec_restart_if_pending(&app, cli);

    if let Err(e) = run_result {
        drop(writeln!(io::stderr(), "Fatal: {e}"));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    init_file_logger();
    raise_fd_limit();
    infra::flame::init();

    let cli = parse_cli();
    if cli.headless { headless_main(&cli) } else { tui_main(&cli) }
}
