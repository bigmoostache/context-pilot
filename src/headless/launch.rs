//! Headless launch helpers — daemon boot, client attach, session listing.
//!
//! Called from `main()` based on CLI flags. Each function is a
//! self-contained entry point that exits the process when done.

use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crate::app::{App, ensure_default_agent, ensure_default_contexts};
use crate::headless::{client::HeadlessClient, server::SocketServer, session};
use crate::infra::api::StreamEvent;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::{
    boot_assemble_state, boot_extract_module_data, boot_init_modules, boot_load_config, boot_load_messages,
    boot_load_panels, load_state,
};

/// Daemon shutdown flag — set by SIGTERM/SIGINT signal handlers.
///
/// Checked each tick by [`App::run_daemon`] to trigger graceful shutdown
/// (save state, notify clients, cleanup socket/PID files).
pub(crate) static DAEMON_SHUTDOWN: std::sync::LazyLock<Arc<AtomicBool>> =
    std::sync::LazyLock::new(|| Arc::new(AtomicBool::new(false)));

/// Returns `true` if a shutdown signal has been received.
pub(crate) fn is_shutdown_requested() -> bool {
    DAEMON_SHUTDOWN.load(Ordering::Relaxed)
}

// ── Daemon (--daemon-internal) ───────────────────────────────────

/// Boot the daemon process: load state, start socket server, run event loop.
///
/// This is the hidden `--daemon-internal` code path. The process has
/// already been spawned with stdin/stdout/stderr redirected — no
/// terminal interaction happens here.
pub(crate) fn run_daemon(resume_stream: bool) -> ExitCode {
    init_daemon_infra();

    let project_path = match std::env::current_dir().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(e) => {
            log::error!("headless: failed to resolve project path: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Write PID + project_path so --list and --attach can discover us
    if let Err(e) = session::write_session_files(&project_path, std::process::id()) {
        log::error!("headless: failed to write session files: {e}");
        return ExitCode::FAILURE;
    }

    // ── Phased boot (without terminal / boot screen) ─────────
    let state = boot_headless_state();

    // ── Socket server ────────────────────────────────────────
    let socket_path = session::socket_path(&project_path);
    let mut server = match SocketServer::bind(&socket_path) {
        Ok(s) => s,
        Err(e) => {
            log::error!("headless: failed to bind socket at {}: {e}", socket_path.display());
            session::remove_session(&project_path);
            return ExitCode::FAILURE;
        }
    };

    log::info!("headless: daemon ready, socket at {}", socket_path.display());

    // ── App + event loop ─────────────────────────────────────
    let (tx, rx) = mpsc::channel::<StreamEvent>();
    let (cache_tx, cache_rx) = mpsc::channel::<CacheUpdate>();
    let mut app = App::new(state, cache_tx, resume_stream);
    let ch = crate::app::run::lifecycle::EventChannels { tx: &tx, rx: &rx, cache_rx: &cache_rx };

    let result = app.run_daemon(&mut server, &ch);

    // ── Cleanup ──────────────────────────────────────────────
    server.shutdown();
    session::remove_session(&project_path);
    crate::infra::flame::flush();

    // Self-restart on reload (same as main.rs)
    #[cfg(unix)]
    if app.state.flags.lifecycle.reload_pending && std::env::var_os("CP_RUN_SH").is_none() {
        if let Ok(exe_path) = std::env::current_exe() {
            use std::os::unix::process::CommandExt as _;
            let mut args: Vec<String> = std::env::args().skip(1).collect();
            if !args.iter().any(|a| a == "--resume-stream") {
                args.push("--resume-stream".to_string());
            }
            let _err = std::process::Command::new(exe_path).args(&args).exec();
        }
    }

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("headless: daemon error: {e}");
            ExitCode::FAILURE
        }
    }
}

// ── Attach (--attach) ────────────────────────────────────────────

/// Attach to a running daemon for the current project directory.
pub(crate) fn run_attach() -> ExitCode {
    let project_path = match std::env::current_dir().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(e) => {
            drop(writeln!(io::stderr(), "Error: failed to resolve project path: {e}"));
            return ExitCode::FAILURE;
        }
    };

    if !session::is_daemon_running(&project_path) {
        // Clean up stale session if daemon died
        let _ = session::cleanup_stale_session(&project_path);
        drop(writeln!(io::stderr(), "No daemon running for this project directory."));
        drop(writeln!(io::stderr(), "Start one with: cpilot --daemon-internal"));
        return ExitCode::FAILURE;
    }

    let socket_path = session::socket_path(&project_path);
    let mut client = match HeadlessClient::connect(&socket_path) {
        Ok(c) => c,
        Err(e) => {
            drop(writeln!(io::stderr(), "Failed to connect to daemon: {e}"));
            return ExitCode::FAILURE;
        }
    };

    match client.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            drop(writeln!(io::stderr(), "Client error: {e}"));
            ExitCode::FAILURE
        }
    }
}

// ── List (--list) ────────────────────────────────────────────────

/// List all active daemon sessions and exit.
pub(crate) fn run_list() -> ExitCode {
    let sessions = session::list_sessions();

    if sessions.is_empty() {
        drop(writeln!(io::stdout(), "No active daemon sessions."));
        return ExitCode::SUCCESS;
    }

    drop(writeln!(io::stdout(), "  {:<50} {:<8} {}", "PATH", "PID", "STATUS"));
    drop(writeln!(io::stdout(), "  {}", "─".repeat(70)));

    for info in &sessions {
        drop(writeln!(io::stdout(), "  {:<50} {:<8} running", info.project_path, info.pid));
    }

    ExitCode::SUCCESS
}

// ── Stop (--stop) ────────────────────────────────────────────────

/// Gracefully stop the daemon for the current project directory.
pub(crate) fn run_stop() -> ExitCode {
    let project_path = match std::env::current_dir().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(e) => {
            drop(writeln!(io::stderr(), "Error: failed to resolve project path: {e}"));
            return ExitCode::FAILURE;
        }
    };

    if !session::is_daemon_running(&project_path) {
        let _ = session::cleanup_stale_session(&project_path);
        drop(writeln!(io::stderr(), "No daemon running for this project directory."));
        return ExitCode::SUCCESS;
    }

    // Connect and send Quit message
    let socket_path = session::socket_path(&project_path);
    match std::os::unix::net::UnixStream::connect(&socket_path) {
        Ok(stream) => {
            let mut writer = BufWriter::new(stream);
            if let Err(e) =
                crate::headless::protocol::write_message(&mut writer, &crate::headless::protocol::ClientMessage::Quit)
            {
                drop(writeln!(io::stderr(), "Failed to send stop command: {e}"));
                return ExitCode::FAILURE;
            }
            drop(writer.flush());
            drop(writeln!(io::stdout(), "Stop signal sent. Daemon shutting down."));
            ExitCode::SUCCESS
        }
        Err(e) => {
            drop(writeln!(io::stderr(), "Failed to connect to daemon: {e}"));
            let _ = session::cleanup_stale_session(&project_path);
            ExitCode::FAILURE
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────

/// Standard launch: spawn daemon + auto-attach ("spawn-and-become").
///
/// If a daemon is already running for this project, skips spawning and
/// attaches directly. Otherwise, spawns `cpilot --daemon-internal` as a
/// background process with stdout/stderr redirected to `daemon.log`,
/// waits for the socket to appear, then transitions into attach mode.
pub(crate) fn run_standard_launch(resume_stream: bool) -> ExitCode {
    let project_path = match std::env::current_dir().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(e) => {
            drop(writeln!(io::stderr(), "Error: failed to resolve project path: {e}"));
            return ExitCode::FAILURE;
        }
    };

    // If a daemon is already running, just attach
    if session::is_daemon_running(&project_path) {
        return run_attach();
    }

    // Clean up any stale session from a dead daemon
    let _ = session::cleanup_stale_session(&project_path);

    // Resolve our own executable for respawning
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            drop(writeln!(io::stderr(), "Error: cannot find executable path: {e}"));
            return ExitCode::FAILURE;
        }
    };

    // Ensure session directory exists before creating the log file
    let log_path = session::log_path(&project_path);
    if let Some(parent) = log_path.parent() {
        drop(std::fs::create_dir_all(parent));
    }

    // Open log file for daemon stdout/stderr redirect
    let log_file = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            drop(writeln!(io::stderr(), "Error: cannot create daemon log at {}: {e}", log_path.display()));
            return ExitCode::FAILURE;
        }
    };
    let log_stderr = match log_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            drop(writeln!(io::stderr(), "Error: cannot duplicate log file handle: {e}"));
            return ExitCode::FAILURE;
        }
    };

    // Spawn the daemon as a detached background process
    let mut cmd = std::process::Command::new(&exe_path);
    let _ = cmd.arg("--daemon-internal");
    if resume_stream {
        let _ = cmd.arg("--resume-stream");
    }
    let _ = cmd.stdout(log_file).stderr(log_stderr).stdin(std::process::Stdio::null());

    match cmd.spawn() {
        Ok(_child) => {
            // Don't wait — the child IS the daemon. Poll for its socket.
            let socket_path = session::socket_path(&project_path);
            if !wait_for_socket(&socket_path, 15) {
                drop(writeln!(io::stderr(), "Timeout: daemon failed to start within 15 seconds."));
                drop(writeln!(io::stderr(), "Check daemon log: {}", log_path.display()));
                return ExitCode::FAILURE;
            }

            // Brief pause so the listener is fully ready to accept
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Become the attach client
            run_attach()
        }
        Err(e) => {
            drop(writeln!(io::stderr(), "Error: failed to spawn daemon process: {e}"));
            ExitCode::FAILURE
        }
    }
}

/// Poll for a Unix socket file to appear, with geometric backoff.
///
/// Starts at 25 ms, doubles each iteration up to 500 ms, gives up after
/// `timeout_secs` seconds. Returns `true` if the socket appeared.
fn wait_for_socket(socket_path: &Path, timeout_secs: u64) -> bool {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let max_delay = std::time::Duration::from_millis(500);
    let mut delay = std::time::Duration::from_millis(25);

    while !socket_path.exists() {
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(delay);
        // Geometric backoff: 25 → 50 → 100 → 200 → 400 → 500 (capped)
        delay = delay.saturating_mul(2).min(max_delay);
    }
    true
}

/// Shared daemon infrastructure init (logger, FD limits, flame graph, signals).
fn init_daemon_infra() {
    crate::init_file_logger();
    crate::raise_fd_limit();
    crate::infra::flame::init();
    install_daemon_signals();
}

/// Register signal handlers for the daemon process.
///
/// - **SIGTERM / SIGINT** → set [`DAEMON_SHUTDOWN`] flag (graceful exit)
/// - **SIGHUP** → ignore (terminal hangup irrelevant for a daemon)
fn install_daemon_signals() {
    // SIGTERM + SIGINT → graceful shutdown
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        drop(signal_hook::flag::register(sig, Arc::clone(&DAEMON_SHUTDOWN)));
    }

    // SIGHUP → swallow silently (flag set but never checked)
    let hup_sink = Arc::new(AtomicBool::new(false));
    drop(signal_hook::flag::register(signal_hook::consts::SIGHUP, hup_sink));
}

/// Run the full phased boot sequence without a terminal or boot screen.
///
/// Returns the assembled [`State`] ready for the daemon event loop.
fn boot_headless_state() -> crate::state::State {
    let new_format = Path::new(".context-pilot").join("config.json").exists();

    let mut state = if new_format {
        let cfg = boot_load_config();
        let module_data = boot_extract_module_data(&cfg);
        let panels = boot_load_panels(&cfg);
        let messages = boot_load_messages(&panels.message_uids);
        let mut assembled = boot_assemble_state(cfg, panels, messages);
        boot_init_modules(&mut assembled, &module_data, |_| {});
        assembled
    } else {
        load_state()
    };

    state.highlight_ir_fn = Some(crate::ui::helpers::highlight_file_ir);
    crate::modules::validate_dependencies(&state.active_modules);
    crate::modules::init_registry();

    // Remove orphaned context elements
    {
        let known_types: std::collections::HashSet<String> = crate::modules::all_modules()
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

    ensure_default_contexts(&mut state);
    ensure_default_agent(&mut state);

    state
}
