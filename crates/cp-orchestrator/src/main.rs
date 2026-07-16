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
use minisign_verify as _;
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
    // Arguments must be handled before ANYTHING boots: a silently-ignored
    // `--version` used to start the full server, bind the port, and shadow
    // the real service (M6 e2e, 2026-07-16). Unknown arguments are a hard
    // error for the same reason.
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("cp-orchestrator v{} (protocol v{})", env!("CARGO_PKG_VERSION"), cp_wire::PROTOCOL_VERSION);
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

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

    // Reconcile a rolled-back update (update-policy §5.5 step 6) BEFORE the
    // auth store opens: if a staged update crash-looped and `boot_check`
    // restored the old binary, this restores the matching `auth.db` backup (a
    // forward migration may have run, §5.8) and records `rolled_back`.
    if let Some(install) = install.as_deref() {
        let releases_dir = cp_orchestrator::services::ReleaseStore::default_dir()
            .unwrap_or_else(|| config.agents_dir.join("releases"));
        cp_orchestrator::services::releases::updater::boot_reconcile(&releases_dir, &config.auth_db_path, install);
    }

    eprintln!("agents directory: {}", config.agents_dir.display());
    eprintln!("scan interval: {}ms", config.scan_interval.as_millis());
    eprintln!("new-agent realm root: {}", config.agents_root.display());
    eprintln!("agent binary: {}", config.agent_binary.display());

    let runtime = Runtime::new(config);
    let _driver = runtime.start_driver();

    // Health-gated commit of a staged update (update-policy §5.5): a committer
    // thread polls our own `/healthz` and, only after a real `200` within the
    // deadline, commits the binary swap and promotes the release state
    // (`active_tag`, agent binary). If the probe never turns healthy, the
    // rollback markers stay and the next boot's `boot_check` self-heals.
    if let Some(install) = install {
        let _committer = runtime.start_update_committer(install.clone());
        // Auto-update scheduler (O4.2): boot poll + nightly-window applies.
        let _scheduler = runtime.start_update_scheduler(install);
    }

    if let Err(e) = runtime.serve() {
        eprintln!("serve failed: {e}");
        std::process::exit(1);
    }
}
