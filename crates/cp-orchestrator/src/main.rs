//! Standalone orchestration backend binary — discovers, observes, and commands
//! a fleet of Context Pilot agents.
//!
//! Reads configuration from environment variables (see [`runtime::Config`]),
//! spawns a background driver thread that scans the registry and tails every
//! agent's oplog, then blocks on the HTTP transport serving REST + SSE.

use cp_orchestrator::runtime::{Config, Runtime};

// Acknowledge crate-level dependencies used only by the library half or by
// dev-dependencies linked into the bin-test target.
use argon2 as _;
use base64 as _;
use calamine as _;
use cp_base as _;
#[cfg(test)]
use cp_mod_bridge as _;
use cp_oplog as _;
use cp_vault as _;
use csv as _;
use nix as _;
use notify as _;
use openssl as _;
use portable_pty as _;
use reqwest as _;
use rusqlite as _;
use serde as _;
use serde_json as _;
use serde_yaml as _;
use sha2 as _;
#[cfg(test)]
use tempfile as _;
use tiny_http as _;
use utoipa as _;

fn main() {
    // Load .env files — project-local first, then global fallback.
    let _local = dotenvy::dotenv().ok();
    if let Some(home) = std::env::var_os("HOME") {
        let global_env = std::path::PathBuf::from(home).join(".context-pilot/.env");
        let _global = dotenvy::from_path(&global_env).ok();
    }

    eprintln!("cp-orchestrator v{} (protocol v{})", env!("CARGO_PKG_VERSION"), cp_wire::PROTOCOL_VERSION,);

    // Self-update guard. If the "Update & Restart Orchestrator" button staged a
    // new binary over our install path, a `.pending` marker is present. Account
    // for this boot attempt *before* we bind anything: if the staged binary has
    // crash-looped past the tolerance, `boot_check` rolls back to the `.bak`
    // binary so the service self-heals. Once we've stayed up past a short grace
    // period, a watchdog thread commits the update (clears the marker + backup).
    if let Ok(install) = std::env::current_exe() {
        cp_orchestrator::services::releases::boot_check(&install);
        let _watchdog = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            cp_orchestrator::services::releases::boot_commit(&install);
        });
    }

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("agents directory: {}", config.agents_dir.display());
    eprintln!("scan interval: {}ms", config.scan_interval.as_millis());
    eprintln!("new-agent realm root: {}", config.agents_root.display());
    eprintln!("agent binary: {}", config.agent_binary.display());

    let runtime = Runtime::new(config);
    let _driver = runtime.start_driver();

    if let Err(e) = runtime.serve() {
        eprintln!("serve failed: {e}");
        std::process::exit(1);
    }
}
