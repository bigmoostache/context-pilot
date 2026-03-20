//! Tuwunel homeserver process lifecycle management.
//!
//! Handles starting, stopping, and health-checking the local Matrix
//! homeserver that runs alongside Context Pilot. The server listens
//! on `127.0.0.1:6167` (localhost only, no federation by default).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cp_base::state::runtime::State;

use crate::types::{ChatState, ServerStatus};

/// Default address the homeserver listens on.
pub(crate) const SERVER_ADDR: &str = "127.0.0.1:6167";

/// Maximum time to wait for the server to become healthy after start.
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Interval between health check retries during startup.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// Grace period before force-killing after requesting termination.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Path to the Tuwunel binary relative to the user's home directory.
const BIN_REL_PATH: &str = ".context-pilot/bin/tuwunel";

/// Runtime handle for the Tuwunel child process.
///
/// Kept outside `ChatState` because `Child` is neither `Clone` nor
/// `Serialize`. The PID is mirrored into `ChatState.server_pid` for
/// display and persistence purposes.
static SERVER_CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// Returns the absolute path to the Tuwunel binary.
#[must_use]
pub(crate) fn binary_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(BIN_REL_PATH))
}

/// Returns the matrix data directory inside the project's `.context-pilot/`.
#[must_use]
pub(crate) fn data_dir(project_root: &Path) -> PathBuf {
    project_root.join(".context-pilot/matrix")
}

/// Returns the path to the homeserver config file.
#[must_use]
pub(crate) fn config_path(project_root: &Path) -> PathBuf {
    data_dir(project_root).join("homeserver.toml")
}

/// Start the Tuwunel homeserver as a managed child process.
///
/// 1. Verifies the binary exists at `~/.context-pilot/bin/tuwunel`.
/// 2. Verifies `homeserver.toml` exists (run bootstrap first).
/// 3. Spawns the process with stdout/stderr redirected to `server.log`.
/// 4. Polls the health endpoint until it responds or times out (15 s).
///
/// # Errors
///
/// Returns a descriptive error if the binary is missing, config is
/// absent, the process fails to spawn, or health check times out.
pub(crate) fn start_server(state: &mut State) -> Result<(), String> {
    // Phase 1: validate prerequisites and spawn the server process
    let child = {
        let cs = ChatState::get_mut(state);

        // Already running — nothing to do
        if cs.server_status == ServerStatus::Running {
            return Ok(());
        }

        cs.server_status = ServerStatus::Starting;

        let bin = binary_path().ok_or("Cannot determine home directory for Tuwunel binary")?;
        if !bin.exists() {
            cs.server_status = ServerStatus::Stopped;
            return Err(format!("Tuwunel binary not found at {}. Install it first.", bin.display()));
        }

        let root = Path::new(".");
        let data = data_dir(root);
        let cfg = config_path(root);
        if !cfg.exists() {
            cs.server_status = ServerStatus::Stopped;
            return Err(format!("Config not found at {}. Run bootstrap first.", cfg.display()));
        }

        let log_path = data.join("server.log");
        let log_file = std::fs::File::create(&log_path).map_err(|e| {
            cs.server_status = ServerStatus::Error(e.to_string());
            format!("Cannot create server log at {}: {e}", log_path.display())
        })?;
        let log_err = log_file.try_clone().map_err(|e| {
            cs.server_status = ServerStatus::Error(e.to_string());
            format!("Cannot duplicate log file handle: {e}")
        })?;

        let child = Command::new(&bin)
            .arg("--config")
            .arg(&cfg)
            .stdin(Stdio::null())
            .stdout(log_file)
            .stderr(log_err)
            .spawn()
            .map_err(|e| {
                cs.server_status = ServerStatus::Error(e.to_string());
                format!("Failed to spawn Tuwunel: {e}")
            })?;

        let pid = child.id();
        cs.server_pid = Some(pid);

        child
    };

    // Stash the Child handle for later shutdown
    if let Ok(mut guard) = SERVER_CHILD.lock() {
        *guard = Some(child);
    }

    // Phase 2: wait for health and run post-start setup
    let health = wait_for_health();

    let cs = ChatState::get_mut(state);
    match health {
        Ok(()) => cs.server_status = ServerStatus::Running,
        Err(ref e) => {
            cs.server_status = ServerStatus::Error(e.clone());
            return Err(e.clone());
        }
    }

    // Register bot account + create default room on first boot
    if let Err(e) = crate::bootstrap::post_start_setup(state) {
        log::warn!("Post-start setup incomplete: {e}");
    }

    Ok(())
}

/// Stop the Tuwunel homeserver gracefully.
///
/// Sends SIGTERM via the `kill` command, waits up to 5 seconds for
/// exit, then force-kills if still alive. Updates `ChatState` status.
pub(crate) fn stop_server(state: &mut State) {
    let cs = ChatState::get_mut(state);

    if let Ok(mut guard) = SERVER_CHILD.lock() {
        if let Some(ref mut child) = *guard {
            // Request graceful shutdown via `kill` command (safe, no unsafe)
            {
                let _r = Command::new("kill").arg(child.id().to_string()).status();
            }

            // Wait for exit within the grace period
            let deadline = Instant::now().checked_add(SHUTDOWN_GRACE);
            loop {
                match child.try_wait() {
                    Ok(None) if deadline.is_some_and(|d| Instant::now() >= d) => {
                        // Grace period expired — force kill
                        {
                            let _r = child.kill();
                        }
                        {
                            let _r = child.wait();
                        }
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                    Ok(Some(_)) | Err(_) => break,
                }
            }
        }
        *guard = None;
    }

    cs.server_pid = None;
    cs.server_status = ServerStatus::Stopped;
}

/// Check if the homeserver is healthy by hitting the versions endpoint.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or returns a non-`2xx` status.
pub(crate) fn health_check() -> Result<(), String> {
    let url = format!("http://{SERVER_ADDR}/_matrix/client/versions");
    let resp = reqwest::blocking::get(&url).map_err(|e| format!("Health check failed: {e}"))?;
    if resp.status().is_success() { Ok(()) } else { Err(format!("Health check returned HTTP {}", resp.status())) }
}

/// Poll the health endpoint until it responds or the timeout expires.
fn wait_for_health() -> Result<(), String> {
    let deadline = Instant::now().checked_add(HEALTH_CHECK_TIMEOUT);
    loop {
        if health_check().is_ok() {
            return Ok(());
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(format!("Tuwunel did not become healthy within {}s", HEALTH_CHECK_TIMEOUT.as_secs()));
        }
        std::thread::sleep(HEALTH_CHECK_INTERVAL);
    }
}
