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
    // Load .env files — override mode so file values always win over stale
    // shell env vars (e.g. BRAVE_API_KEY inherited from parent process).
    // Global loads second and overrides project-local — it's where the
    // settings-page vault.set() writes, so it has the latest user intent.
    let _local = dotenvy::dotenv_override().ok();
    if let Some(home) = std::env::var_os("HOME") {
        let global_env = std::path::PathBuf::from(home).join(".context-pilot/.env");
        let _global = dotenvy::from_path_override(&global_env).ok();
    }

    eprintln!("cp-orchestrator v{} (protocol v{})", env!("CARGO_PKG_VERSION"), cp_wire::PROTOCOL_VERSION,);

    // Self-update guard. If a staged update replaced the binary on our install
    // path, a `.pending` marker is present. Account for this boot attempt
    // *before* we bind anything: if the staged binary has crash-looped past the
    // tolerance, `boot_check` rolls back to the `.bak` binary so the service
    // self-heals. The matching commit is health-gated below (needs the port).
    let install = std::env::current_exe().ok();
    if let Some(install) = install.as_deref() {
        cp_orchestrator::services::releases::boot_check(install);
    }

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(1);
        }
    };

    // Health-gated commit of a staged update (update-policy §5.5): a committer
    // thread polls our own `/healthz` and clears the rollback markers only
    // after a real `200` — socket bound + auth DB answering + registry
    // readable — within the deadline. If the probe never turns healthy, the
    // markers stay and the next boot's `boot_check` counts the failure.
    if let Some(install) = install {
        let url = format!("http://127.0.0.1:{}/healthz", config.port);
        let _committer = std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new());
            let healthy = || client.get(&url).send().map(|r| r.status().as_u16() == 200).unwrap_or(false);
            let _committed = cp_orchestrator::services::releases::boot_commit_when_healthy(
                &install,
                healthy,
                std::time::Duration::from_secs(60),
                std::time::Duration::from_secs(2),
            );
        });
    }

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
