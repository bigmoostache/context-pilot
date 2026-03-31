//! Bridge binary download and process lifecycle management.
//!
//! Handles downloading mautrix bridge binaries from GitHub, spawning
//! them as child processes, PID file tracking, and health checks.
//! Follows the same pattern as Tuwunel's own server lifecycle.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{BRIDGES, bridge_data_dir};

/// Time to wait for a bridge to become healthy after start.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(10);

/// Interval between health check retries during bridge startup.
const HEALTH_INTERVAL: Duration = Duration::from_millis(500);

// -- Binary management -------------------------------------------------------

/// Path to a bridge binary: `~/.context-pilot/bin/mautrix-{name}`
#[must_use]
pub(crate) fn binary_path(name: &str) -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(format!(".context-pilot/bin/mautrix-{name}")))
}

/// Download a bridge binary from GitHub if not already present.
///
/// Uses the `/releases/latest/download/` URL which follows redirects
/// to the most recent release. The binary is a static Go executable
/// (~30–50 MB depending on the bridge).
///
/// # Errors
///
/// Returns a description if the download fails or the architecture
/// is unsupported.
pub(crate) fn ensure_binary(name: &str) -> Result<PathBuf, String> {
    let bin = binary_path(name).ok_or("Cannot determine home directory")?;
    if bin.exists() {
        return Ok(bin);
    }

    let bin_dir = bin.parent().ok_or("Invalid binary path")?;
    std::fs::create_dir_all(bin_dir).map_err(|e| format!("Cannot create {}: {e}", bin_dir.display()))?;

    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => return Err(format!("Unsupported architecture for mautrix bridges: {other}")),
    };

    let url = format!("https://github.com/mautrix/{name}/releases/latest/download/mautrix-{name}-{arch}");
    log::info!("Downloading mautrix-{name} from {url}...");

    let resp = reqwest::blocking::get(&url).map_err(|e| format!("Download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Download returned HTTP {}", resp.status()));
    }

    let bytes = resp.bytes().map_err(|e| format!("Failed to read response: {e}"))?;
    std::fs::write(&bin, &bytes).map_err(|e| format!("Cannot write {}: {e}", bin.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&bin, perms).map_err(|e| format!("Cannot chmod: {e}"))?;
    }

    log::info!("mautrix-{name} installed at {} ({} bytes)", bin.display(), bytes.len());
    Ok(bin)
}

// -- Registration generation -------------------------------------------------

/// Generate `registration.yaml` for all bridges that have a config but
/// no registration yet. Runs `mautrix-{name} -g` which populates both
/// the registration file AND the `as_token`/`hs_token` in config.yaml.
///
/// # Errors
///
/// Returns a description if binary download or generation fails.
pub(crate) fn generate_registrations() -> Result<(), String> {
    for spec in BRIDGES {
        let data_dir = bridge_data_dir(spec.name);
        let cfg_path = data_dir.join("config.yaml");
        let reg_path = data_dir.join("registration.yaml");

        // Skip if registration already generated
        if reg_path.exists() {
            continue;
        }

        // Seed a minimal config if none exists — mautrix -g needs *something*
        // to read. Our real config (with tokens, DB, etc.) overwrites this
        // after -g finishes via generate_bridge_configs().
        if !cfg_path.exists() {
            std::fs::create_dir_all(&data_dir).map_err(|e| format!("Cannot create {}: {e}", data_dir.display()))?;
            std::fs::write(&cfg_path, "{}\n")
                .map_err(|e| format!("Cannot seed config for mautrix-{}: {e}", spec.name))?;
        }

        let bin = ensure_binary(spec.name)?;

        log::info!("Generating registration for mautrix-{}...", spec.name);
        let output = Command::new(&bin)
            .arg("--generate-registration")
            .arg("--config")
            .arg(&cfg_path)
            .arg("--registration")
            .arg(&reg_path)
            .current_dir(&data_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Failed to run mautrix-{} -g: {e}", spec.name))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("mautrix-{} registration generation failed: {stderr}", spec.name));
        }

        if !reg_path.exists() {
            return Err(format!("mautrix-{} -g completed but {} was not created", spec.name, reg_path.display()));
        }

        // Note: -g clobbers config.yaml with mautrix defaults. That's fine —
        // generate_bridge_configs() runs AFTER this and overwrites with our
        // config (including as_token/hs_token read from registration.yaml).

        log::info!("Registration generated for mautrix-{} at {}", spec.name, reg_path.display());
    }

    Ok(())
}

// -- Process lifecycle -------------------------------------------------------

/// PID file path: `~/.context-pilot/matrix/bridges/{name}/bridge.pid`
fn pid_path(name: &str) -> PathBuf {
    bridge_data_dir(name).join("bridge.pid")
}

/// Read a bridge's PID from its PID file.
fn read_pid(name: &str) -> Option<u32> {
    let path = pid_path(name);
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Check if a process is alive (kill -0).
fn is_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Start a bridge process.
///
/// Downloads the binary if needed, then spawns it with the config file
/// in the bridge's data directory. Writes a PID file for orphan recovery.
///
/// # Errors
///
/// Returns a description if the binary can't be obtained, the config
/// is missing, or the process fails to spawn.
pub(crate) fn start(name: &str) -> Result<u32, String> {
    let spec = BRIDGES.iter().find(|b| b.name == name).ok_or_else(|| format!("Unknown bridge: {name}"))?;

    // Orphan recovery — reuse an existing healthy process
    if let Some(pid) = read_pid(name) {
        if is_alive(pid) && health_check(spec.appservice_port).is_ok() {
            log::info!("Reconnected to existing mautrix-{name} (PID {pid})");
            return Ok(pid);
        }
        let _r = std::fs::remove_file(pid_path(name));
    }

    let bin = ensure_binary(name)?;

    let data_dir = bridge_data_dir(name);
    let cfg_path = data_dir.join("config.yaml");
    if !cfg_path.exists() {
        return Err(format!("Bridge config not found at {}. Run bootstrap first.", cfg_path.display()));
    }

    // Ensure registration exists — generate if missing
    let reg_path = data_dir.join("registration.yaml");
    if !reg_path.exists() {
        log::info!("No registration.yaml for mautrix-{name}, generating...");
        generate_registrations()?;
        if !reg_path.exists() {
            return Err(format!("Registration file missing for mautrix-{name}. Cannot start bridge."));
        }
    }

    let log_path = data_dir.join("bridge.log");
    let log_file =
        std::fs::File::create(&log_path).map_err(|e| format!("Cannot create {}: {e}", log_path.display()))?;
    let log_err = log_file.try_clone().map_err(|e| format!("Cannot dup log handle: {e}"))?;

    let child = Command::new(&bin)
        .arg("--config")
        .arg(&cfg_path)
        .current_dir(&data_dir)
        .stdin(Stdio::null())
        .stdout(log_file)
        .stderr(log_err)
        .spawn()
        .map_err(|e| format!("Failed to spawn mautrix-{name}: {e}"))?;

    let pid = child.id();
    let _r = std::fs::write(pid_path(name), pid.to_string());

    // Wait for the bridge to become healthy
    let deadline = Instant::now().checked_add(HEALTH_TIMEOUT);
    loop {
        if health_check(spec.appservice_port).is_ok() {
            log::info!("mautrix-{name} healthy (PID {pid}, port {})", spec.appservice_port);
            return Ok(pid);
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            // Don't kill — first-run database migrations can be slow
            log::warn!("mautrix-{name} health check timed out, leaving process running");
            return Ok(pid);
        }
        std::thread::sleep(HEALTH_INTERVAL);
    }
}

// -- Health check ------------------------------------------------------------

/// Health-check a bridge by hitting its appservice port.
///
/// Mautrix bridges respond on `/_matrix/app/v1/ping`. A 200 or 401
/// (no auth token) both indicate the bridge is alive and listening.
fn health_check(port: u16) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/_matrix/app/v1/ping");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    let resp = client
        .post(&url)
        .json(&serde_json::json!({}))
        .send()
        .map_err(|e| format!("Bridge health check failed: {e}"))?;
    if resp.status().is_success() || resp.status().as_u16() == 401 {
        Ok(())
    } else {
        Err(format!("Bridge returned HTTP {}", resp.status()))
    }
}

// -- YAML helpers ------------------------------------------------------------

/// Extract a top-level value from a simple YAML string.
///
/// Looks for `key: value` lines and returns the trimmed value with any
/// surrounding quotes stripped. Only handles flat YAML — no nesting.
pub(crate) fn extract_yaml_value(yaml: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    yaml.lines()
        .find(|l| l.trim_start().starts_with(&prefix))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().trim_matches('"').trim_matches('\'').to_owned())
}
