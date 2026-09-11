//! Standalone orchestration backend binary — discovers, observes, and commands
//! a fleet of Context Pilot agents.
//!
//! Reads configuration from environment variables (validated by `cp-env`, see
//! `docs/ENV.md`; typed view in [`runtime::Config`]),
//! spawns a background driver thread that scans the registry and tails every
//! agent's oplog, then blocks on the HTTP transport serving REST + SSE.

use std::process::ExitCode;

use cp_orchestrator::runtime::{Config, Runtime};

// Acknowledge crate-level dependencies used only by the library half or by
// dev-dependencies linked into the bin-test target.
use argon2 as _;
use base64 as _;
use calamine as _;
use cp_base as _;
use cp_env::spec::Target;
#[cfg(test)]
use cp_mod_bridge as _;
use cp_mod_utilities as _;
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

fn main() -> ExitCode {
    if let Some(exit) = early_exit_for_args() {
        return exit;
    }

    load_dotenv_files();

    cp_orchestrator::oerr!("cp-orchestrator v{} (protocol v{})", env!("CARGO_PKG_VERSION"), cp_wire::PROTOCOL_VERSION);

    if let Some(exit) = validate_environment() {
        return exit;
    }

    // Self-update guard. If a staged update replaced the binary on our install
    // path, a `.pending` marker is present. Account for this boot attempt
    // *before* we bind anything: if the staged binary has crash-looped past the
    // tolerance, `boot_check` rolls back to the `.bak` binary so the service
    // self-heals. The matching commit is health-gated below (needs the port).
    let install_path = std::env::current_exe().ok();
    if let Some(install) = install_path.as_deref() {
        cp_orchestrator::services::releases::self_update::boot_check(install);
    }

    let config = Config::view(cp_env::env());

    // Reconcile a rolled-back update (update-policy §5.5 step 6) BEFORE the
    // auth store opens: if a staged update crash-looped and `boot_check`
    // restored the old binary, this restores the matching `auth.db` backup (a
    // forward migration may have run, §5.8) and records `rolled_back`.
    if let Some(install) = install_path.as_deref() {
        let releases_dir = cp_orchestrator::services::releases::ReleaseStore::default_dir();
        cp_orchestrator::services::releases::updater::apply::boot_reconcile(
            &releases_dir,
            &config.auth_db_path,
            install,
        );
    }

    cp_orchestrator::oerr!("agents directory: {}", config.agents_dir.display());
    cp_orchestrator::oerr!("scan interval: {}ms", config.scan_interval.as_millis());
    cp_orchestrator::oerr!("new-agent realm root: {}", config.agents_root.display());
    cp_orchestrator::oerr!("agent binary: {}", config.agent_binary.display());

    let runtime = Runtime::new(config);
    let _driver = runtime.start_driver();

    // Keep every Claude OAuth account (active + stored) auto-refreshed, so a
    // token never expires from under the fleet regardless of any open UI.
    let _oauth_sweeper = Runtime::start_oauth_sweeper();

    // Health-gated commit of a staged update (update-policy §5.5): a committer
    // thread polls our own `/healthz` and, only after a real `200` within the
    // deadline, commits the binary swap and promotes the release state
    // (`active_tag`, agent binary). If the probe never turns healthy, the
    // rollback markers stay and the next boot's `boot_check` self-heals.
    if let Some(install) = install_path {
        let _committer = runtime.start_update_committer(install.clone());
        // Auto-update scheduler (O4.2): boot poll + nightly-window applies.
        let _scheduler = runtime.start_update_scheduler(install);
    }

    if let Err(e) = runtime.serve() {
        cp_orchestrator::oerr!("serve failed: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Arguments must be handled before ANYTHING boots: a silently-ignored
/// `--version` used to start the full server, bind the port, and shadow the
/// real service (M6 e2e, 2026-07-16). Unknown arguments are a hard error for
/// the same reason. Only the first argument is meaningful - every branch
/// terminates the process - so it is inspected directly rather than in a loop
/// that could never iterate twice. `None` means "no argument: boot".
fn early_exit_for_args() -> Option<ExitCode> {
    let arg = std::env::args().nth(1)?;
    Some(match arg.as_str() {
        "--version" | "-V" => {
            cp_orchestrator::oout!(
                "cp-orchestrator v{} (protocol v{})",
                env!("CARGO_PKG_VERSION"),
                cp_wire::PROTOCOL_VERSION
            );
            ExitCode::SUCCESS
        }
        // Validate the environment exactly as a boot would (same `.env` merge,
        // same rules), print the report, and exit without binding anything.
        // The systemd unit runs it as `ExecStartPre`.
        "--check-env" => {
            load_dotenv_files();
            let (report, ok) = cp_env::check(Target::Orchestrator);
            cp_orchestrator::oout!("{report}");
            if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
        other => {
            cp_orchestrator::oerr!("unknown argument: {other}");
            ExitCode::from(2)
        }
    })
}

/// Strict validation of the whole environment, before anything else reads it:
/// every problem is reported at once and the process refuses to start
/// (`docs/ENV.md`). On success the effective values go to the journal and the
/// typed configuration is installed for `cp_env::env()`. `Some` is the exit
/// code to return.
fn validate_environment() -> Option<ExitCode> {
    match cp_env::load(Target::Orchestrator) {
        Ok(loaded) => {
            cp_orchestrator::oerr!("{}", loaded.report.trim_end());
            if cp_env::install(loaded.env) {
                None
            } else {
                cp_orchestrator::oerr!("configuration error: environment installed twice");
                Some(ExitCode::FAILURE)
            }
        }
        Err(report) => {
            cp_orchestrator::oerr!("{report}");
            Some(ExitCode::FAILURE)
        }
    }
}

/// Merge the `.env` files into the process environment - override mode so
/// file values always win over stale shell env vars (e.g. `BRAVE_API_KEY`
/// inherited from a parent process). The global file loads second and
/// overrides the project-local one: it is where the settings-page
/// `vault.set()` writes, so it carries the latest user intent.
fn load_dotenv_files() {
    let _local = dotenvy::dotenv_override().ok();
    if let Some(home) = std::env::var_os("HOME") {
        let global_env = std::path::PathBuf::from(home).join(".context-pilot/.env");
        let _global = dotenvy::from_path_override(&global_env).ok();
    }
}
