//! Meilisearch server lifecycle management.
//!
//! Handles downloading, starting, and health-checking a global Meilisearch
//! server at `~/.context-pilot/meilisearch/`. First project starts the server,
//! subsequent projects reuse it via PID + health check.

use std::io::Read as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::download;

// -- Global paths ------------------------------------------------------------

/// Root of all global Meilisearch data: `~/.context-pilot/meilisearch/`.
///
/// `pub(super)` so the watchdog can locate the machine-wide spawn lock beside
/// the pid/port/key files it coordinates with.
pub(super) fn global_meili_dir() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".context-pilot/meilisearch"))
        .map_err(|_e| "Cannot determine HOME directory".to_owned())
}

/// Path to the Meilisearch binary: `~/.context-pilot/meilisearch/bin/meilisearch`.
pub(super) fn binary_path() -> Result<PathBuf, String> {
    global_meili_dir().map(|d| d.join("bin/meilisearch"))
}

/// PID file: `~/.context-pilot/meilisearch/pid`.
fn pid_path() -> Result<PathBuf, String> {
    global_meili_dir().map(|d| d.join("pid"))
}

/// Port file: `~/.context-pilot/meilisearch/port`.
fn port_path() -> Result<PathBuf, String> {
    global_meili_dir().map(|d| d.join("port"))
}

/// Master key file: `~/.context-pilot/meilisearch/master.key`.
fn key_path() -> Result<PathBuf, String> {
    global_meili_dir().map(|d| d.join("master.key"))
}

/// Meilisearch data directory: `~/.context-pilot/meilisearch/data/`.
fn data_dir() -> Result<PathBuf, String> {
    global_meili_dir().map(|d| d.join("data"))
}

/// Projects registry: `~/.context-pilot/meilisearch/projects.json`.
fn projects_path() -> Result<PathBuf, String> {
    global_meili_dir().map(|d| d.join("projects.json"))
}

// -- Directory setup ---------------------------------------------------------

/// Create the global Meilisearch directory tree.
///
/// Creates `~/.context-pilot/meilisearch/` with `bin/` and `data/` subdirs.
///
/// # Errors
///
/// Returns an error if the directories cannot be created.
pub(super) fn ensure_global_dirs() -> Result<PathBuf, String> {
    let root = global_meili_dir()?;

    for sub in &["bin", "data"] {
        let p = root.join(sub);
        std::fs::create_dir_all(&p).map_err(|e| format!("Cannot create {}: {e}", p.display()))?;
    }

    Ok(root)
}

// -- PID management ----------------------------------------------------------

/// Write the server PID to the global PID file.
fn write_pid(pid: u32) -> Result<(), String> {
    let path = pid_path()?;
    std::fs::write(&path, pid.to_string()).map_err(|e| format!("Cannot write PID file {}: {e}", path.display()))
}

/// Read the PID from the global PID file (if it exists).
pub(super) fn read_pid() -> Option<u32> {
    let path = pid_path().ok()?;
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Remove the global PID file.
fn remove_pid() {
    if let Ok(path) = pid_path() {
        let _r = std::fs::remove_file(path);
    }
}

/// Check if a process with the given PID is alive.
///
/// On Linux, reads `/proc/<pid>/status` to exclude zombies.
/// Falls back to `kill -0` on macOS and other platforms.
///
/// `pub(super)` so the watchdog's reconnect check can verify the recorded pid.
pub(super) fn is_pid_alive(pid: u32) -> bool {
    // Try /proc on Linux first
    let proc_status = format!("/proc/{pid}/status");
    if let Ok(content) = std::fs::read_to_string(&proc_status) {
        for line in content.lines() {
            if let Some(state) = line.strip_prefix("State:") {
                let trimmed = state.trim();
                return !trimmed.starts_with('Z') && !trimmed.starts_with('X');
            }
        }
    }

    // Fallback: kill -0
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// -- Port management ---------------------------------------------------------

/// Find a free TCP port by binding to port 0 and reading the assigned port.
///
/// # Errors
///
/// Returns an error if the OS cannot assign a port.
fn find_free_port() -> Result<u16, String> {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Cannot bind to find free port: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("Cannot read assigned port: {e}"))?.port();
    Ok(port)
}

/// Pick the port to bind a (re)spawned server to, preferring the persisted one.
///
/// Stability is the whole point: every agent caches `persist.port` at boot and
/// keeps using it for the session, so if a respawn landed on a *new* random port
/// (the old [`find_free_port`]-always behaviour) those agents would silently
/// query a dead port until their next reload. Here we reuse the port already in
/// the port file whenever it is bindable — which it is in the common
/// death→respawn case, because the dead process released it — so the server
/// comes back at the **same** address and the blip is transparent. We only fall
/// back to a fresh free port on first-ever start (no port file) or if something
/// unrelated has since claimed the old one.
///
/// The test-bind is dropped immediately and meili binds microseconds later; the
/// tiny TOCTOU window is acceptable (nothing else contends for this port).
fn pick_stable_port() -> Result<u16, String> {
    if let Some(p) = read_port()
        && p != 0
        && std::net::TcpListener::bind(("127.0.0.1", p)).is_ok()
    {
        return Ok(p);
    }
    find_free_port()
}

/// Write the server port to the global port file.
fn write_port(port: u16) -> Result<(), String> {
    let path = port_path()?;
    std::fs::write(&path, port.to_string()).map_err(|e| format!("Cannot write port file {}: {e}", path.display()))
}

/// Read the port from the global port file (if it exists).
///
/// `pub(super)` so the watchdog can read the recorded port for reconnect.
pub(super) fn read_port() -> Option<u16> {
    let path = port_path().ok()?;
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

// -- Master key management ---------------------------------------------------

/// Generate a random master key by reading 32 bytes from `/dev/urandom`.
///
/// Returns a 64-character hex string.
///
/// # Errors
///
/// Returns an error if `/dev/urandom` cannot be read.
fn generate_master_key() -> Result<String, String> {
    let mut buf = [0u8; 32];
    let mut f = std::fs::File::open("/dev/urandom").map_err(|e| format!("Cannot open /dev/urandom: {e}"))?;
    f.read_exact(&mut buf).map_err(|e| format!("Cannot read from /dev/urandom: {e}"))?;

    let mut hex = String::with_capacity(64);
    for &b in &buf {
        use std::fmt::Write as _;
        _ = write!(hex, "{b:02x}");
    }
    Ok(hex)
}

/// Write the master key to the global key file.
fn write_master_key(key: &str) -> Result<(), String> {
    let path = key_path()?;
    std::fs::write(&path, key).map_err(|e| format!("Cannot write key file {}: {e}", path.display()))
}

/// Read the master key from the global key file (if it exists).
///
/// `pub(super)` so the watchdog can read the recorded key for reconnect.
pub(super) fn read_master_key() -> Option<String> {
    let path = key_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed = content.trim().to_owned();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

// -- Health check ------------------------------------------------------------

/// Check if the Meilisearch server is healthy.
///
/// Sends `GET /health` with the master key and expects a 200 response.
///
/// # Errors
///
/// Returns an error if the server is unreachable or unhealthy.
fn health_check(port: u16, key: &str) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("Cannot create HTTP client: {e}"))?;

    let resp = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .header("Authorization", format!("Bearer {key}"))
        .send()
        .map_err(|e| format!("Health check failed: {e}"))?;

    if resp.status().is_success() { Ok(()) } else { Err(format!("Health check returned HTTP {}", resp.status())) }
}

/// Boolean health probe over the recorded credentials — the watchdog's tick check.
///
/// A thin `Ok`/`Err` → `bool` wrapper over [`health_check`] so the watchdog loop
/// reads as `if health_ok(..) { continue }`.
pub(super) fn health_ok(port: u16, key: &str) -> bool {
    health_check(port, key).is_ok()
}

/// Whether an already-running server can be reconnected to right now.
///
/// Reads the pid/port/key files and returns `true` only if the recorded process
/// is alive *and* answers a health probe — i.e. exactly the fast-path condition
/// [`ensure_server_running`] uses for Phase 0 reconnect. The watchdog calls this
/// after deferring to another agent's spawn, to confirm the winner brought the
/// server back without spawning anything itself.
pub(super) fn reconnect_ok() -> bool {
    let (Some(port), Some(key), Some(pid)) = (read_port(), read_master_key(), read_pid()) else {
        return false;
    };
    is_pid_alive(pid) && health_check(port, &key).is_ok()
}

/// Poll the health endpoint until the server responds or timeout expires.
///
/// Uses geometric backoff (50 ms → 100 → 200 → 400 → 500 ms cap) so
/// fast startups are detected within ~100 ms instead of the old flat
/// 500 ms interval.  Total budget is still 15 s.
///
/// # Errors
///
/// Returns an error if the server does not become healthy within the timeout.
fn wait_for_health(port: u16, key: &str) -> Result<(), String> {
    let timeout = Duration::from_secs(15);
    let max_interval = Duration::from_millis(500);
    let mut interval = Duration::from_millis(50);
    let deadline = Instant::now().checked_add(timeout);

    loop {
        if health_check(port, key).is_ok() {
            return Ok(());
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(format!("Meilisearch did not become healthy within {timeout:?}"));
        }
        std::thread::sleep(interval);
        interval = (interval.saturating_mul(2)).min(max_interval);
    }
}

// -- Server start/stop -------------------------------------------------------

/// Connection info returned when the server is successfully started.
#[derive(Debug, Clone)]
pub(crate) struct ServerInfo {
    /// TCP port the server is listening on.
    pub port: u16,
    /// Master API key for authentication.
    pub master_key: String,
}

/// Ensure the global Meilisearch server is running.
///
/// 1. Tries to reconnect to an existing server (PID + health check).
/// 2. If not running, downloads the binary (if needed), starts the server,
///    and waits for it to become healthy.
///
/// # Errors
///
/// Returns an error if the server cannot be started.
pub(crate) fn ensure_server_running() -> Result<ServerInfo, String> {
    // Phase 0: try to reconnect to an existing server
    if let (Some(port), Some(key)) = (read_port(), read_master_key())
        && let Some(pid) = read_pid()
    {
        if is_pid_alive(pid) && health_check(port, &key).is_ok() {
            log::info!("Reconnected to existing Meilisearch (PID {pid}, port {port})");
            return Ok(ServerInfo { port, master_key: key });
        }
        // PID is stale — clean up before restarting
        remove_pid();
    }

    // Phase 1: ensure binary exists
    download::download_binary()?;
    let _root = ensure_global_dirs()?;

    // Phase 2: ensure master key exists
    let key = if let Some(k) = read_master_key() {
        k
    } else {
        let k = generate_master_key()?;
        write_master_key(&k)?;
        k
    };

    // Phase 3: pick a stable port (reuse the persisted one if free) and start.
    let port = pick_stable_port()?;
    let bin = binary_path()?;
    let data = data_dir()?;

    let child = Command::new(&bin)
        .arg("--http-addr")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--master-key")
        .arg(&key)
        .arg("--db-path")
        .arg(&data)
        .arg("--env")
        .arg("production")
        .arg("--max-indexing-threads")
        .arg("2")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn Meilisearch: {e}"))?;

    let pid = child.id();

    // Phase 4: write state files
    write_pid(pid)?;
    write_port(port)?;

    // Phase 5: wait for health
    wait_for_health(port, &key)?;

    log::info!("Meilisearch started (PID {pid}, port {port})");

    Ok(ServerInfo { port, master_key: key })
}

// -- Projects registry -------------------------------------------------------

/// Clean up Meilisearch indexes for projects that no longer exist on disk.
///
/// Reads `projects.json`, checks each path, and deletes `cp_{hash}_files`
/// and `cp_{hash}_logs` indexes for missing projects. Writes back the
/// cleaned registry.
///
/// This runs on module init (after server is confirmed healthy).
/// Errors are logged but don't halt startup.
pub(crate) fn cleanup_orphan_indexes(port: u16, master_key: &str) {
    let Ok(path) = projects_path() else {
        return;
    };
    if !path.exists() {
        return;
    }

    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let mut projects: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();

    if projects.is_empty() {
        return;
    }

    // Find orphan entries (project path no longer exists)
    let orphans: Vec<(String, String)> = projects
        .iter()
        .filter_map(|(proj_path, hash_val)| {
            let hash = hash_val.as_str()?;
            (!std::path::Path::new(proj_path).exists()).then(|| (proj_path.clone(), hash.to_owned()))
        })
        .collect();

    if orphans.is_empty() {
        return;
    }

    // Delete indexes for orphan projects
    let Ok(client) = super::api::MeiliClient::new(port, master_key) else {
        return;
    };

    for (proj_path, hash) in &orphans {
        let files_uid = format!("cp_{hash}_files");
        let logs_uid = format!("cp_{hash}_logs");

        // Best-effort deletion — ignore errors (indexes may already be gone)
        if client.delete_index(&files_uid).is_ok() {
            log::info!("Cleaned up orphan index {files_uid} (was: {proj_path})");
        }
        if client.delete_index(&logs_uid).is_ok() {
            log::info!("Cleaned up orphan index {logs_uid} (was: {proj_path})");
        }

        let _removed = projects.remove(proj_path);
    }

    // Write back cleaned projects.json
    if let Ok(json) = serde_json::to_string_pretty(&projects) {
        let _r = std::fs::write(&path, json);
    }
}

/// Register a project in the global projects.json.
///
/// Maps a project path to its 8-character hash so orphan indexes
/// can be cleaned up when projects are removed.
///
/// # Errors
///
/// Returns an error if the file cannot be read or written.
pub(crate) fn register_project(project_path: &str, hash: &str) -> Result<(), String> {
    let path = projects_path()?;

    let mut projects: serde_json::Map<String, serde_json::Value> = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| format!("Cannot read projects.json: {e}"))?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    let _prev = projects.insert(project_path.to_owned(), serde_json::Value::String(hash.to_owned()));

    let json = serde_json::to_string_pretty(&projects).map_err(|e| format!("Cannot serialize projects.json: {e}"))?;

    std::fs::write(&path, json).map_err(|e| format!("Cannot write projects.json: {e}"))
}
