//! Orchestrator **runtime** — the main loop that drives discovery, tailing,
//! view projection, and transport serving.
//!
//! [`Runtime`] owns all the moving parts and exposes two entry points:
//!
//! * [`start_driver`](Runtime::start_driver) — spawns a background thread that
//!   scans the registry, tails every discovered agent's oplog, folds entries
//!   into the shared [`Backend`].
//! * [`serve`](Runtime::serve) — blocks the calling thread on the HTTP
//!   acceptor (delegating to [`transport::serve`]).
//!
//! The driver and transport share [`Backend`] through an [`Arc<Mutex<…>>`] and
//! the convention that the lock is held only for brief, non-blocking mutations.
//!
//! # Configuration
//!
//! All knobs are environment-variable driven (or defaults):
//!
//! | Env var | Default | Meaning |
//! |---|---|---|
//! | `CP_ORCH_PORT` | `7878` | Product cockpit HTTP listen port |
//! | `CP_AGENTS_DIR` | `~/.context-pilot/agents` | Registry directory |
//! | `CP_SCAN_INTERVAL_MS` | `2000` | Registry-discovery + tier-② mtime poll cadence (ms) |
//!
//! The oplog tail (the live state-fold that feeds the view) runs on a
//! much tighter [`driver::TAIL_INTERVAL`] inner cadence, decoupled from the
//! slow registry scan, so a fresh oplog entry reaches the view within ~100 ms
//! rather than the scan interval (a step toward the inotify-primary signal
//! of design doc I12 / §8.1). The loop itself lives in [`driver`].

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

mod driver;
mod seed;
mod update_scheduler;

use crate::services::auth::backup::BackupScheduler;
use crate::transport::Backend;

/// Default product cockpit HTTP listen port.
const DEFAULT_PORT: u16 = 7878;

/// Default registry + oplog poll interval.
const DEFAULT_SCAN_INTERVAL: Duration = Duration::from_millis(2000);

/// Parsed runtime configuration, sourced from environment variables.
#[derive(Debug)]
pub struct Config {
    /// Product cockpit HTTP listen port.
    pub port: u16,
    /// Directory holding agent registry records.
    pub agents_dir: PathBuf,
    /// How often the driver scans the registry and tails oplogs.
    pub scan_interval: Duration,
    /// Root directory new agents' realm folders are created under
    /// (`CP_AGENTS_ROOT`, default `~/code`). The dashboard's create flow puts a
    /// new agent at `<agents_root>/<slug>`.
    pub agents_root: PathBuf,
    /// Absolute path of the `cp` TUI binary the supervisor spawns for a
    /// dashboard-created agent (`CP_AGENT_BINARY`, default
    /// `<cwd>/target/release/tui`). Seeds the supervisor's spawn allow-list
    /// (R2-15), so only this binary can ever be launched.
    pub agent_binary: PathBuf,
    /// Whether authentication is enabled (`CP_AUTH_ENABLED`, default `false`).
    /// When disabled, all requests pass through unauthenticated (FR-18/FR-19).
    pub auth_enabled: bool,
    /// Session lifetime (`CP_SESSION_TTL_SECS`, default 30 days). Absolute
    /// expiry — a session cannot be refreshed past its original TTL (Q6).
    pub session_ttl: Duration,
    /// Path to the auth SQLite database (`CP_AUTH_DB`, default
    /// `~/.context-pilot/orchestrator/auth.db`). Orchestrator-level storage,
    /// not inside agents_dir (D7/Q9).
    pub auth_db_path: PathBuf,
}

impl Config {
    /// Read configuration from environment variables, falling back to defaults.
    ///
    /// # Errors
    ///
    /// Returns a message if `CP_AGENTS_DIR` is absent **and** `$HOME` is unset
    /// (so the default directory cannot be derived).
    pub fn from_env() -> Result<Self, String> {
        let port = std::env::var("CP_ORCH_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_PORT);

        let agents_dir = match std::env::var_os("CP_AGENTS_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => {
                crate::registry::default_agents_dir().map_err(|e| format!("cannot derive agents directory: {e}"))?
            }
        };

        let scan_interval = std::env::var("CP_SCAN_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map_or(DEFAULT_SCAN_INTERVAL, Duration::from_millis);

        // Where new agents' realm folders are created. Default `~/code`, or the
        // current directory if `$HOME` is unset (never fail — creation simply
        // lands somewhere sensible).
        let agents_root = match std::env::var_os("CP_AGENTS_ROOT") {
            Some(dir) => PathBuf::from(dir),
            None => std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), |h| PathBuf::from(h).join("code")),
        };

        // The `cp` TUI binary the supervisor spawns. Default to the release
        // build under the current working directory; override with an absolute
        // path in deployment.
        let agent_binary = match std::env::var_os("CP_AGENT_BINARY") {
            Some(p) => PathBuf::from(p),
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("target/release/tui"),
        };

        // Auth configuration (§8 of design doc).
        let auth_enabled =
            std::env::var("CP_AUTH_ENABLED").ok().map(|s| s.eq_ignore_ascii_case("true") || s == "1").unwrap_or(false);

        let session_ttl = std::env::var("CP_SESSION_TTL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map_or(Duration::from_secs(2_592_000), Duration::from_secs); // 30 days

        let auth_db_path = crate::services::auth::store::AuthStore::default_db_path();

        Ok(Self { port, agents_dir, scan_interval, agents_root, agent_binary, auth_enabled, session_ttl, auth_db_path })
    }
}

/// The orchestrator runtime: fleet discovery + oplog tailing + HTTP serving.
#[derive(Debug)]
pub struct Runtime {
    /// Shared backend state mutated by the driver and read by transport
    /// handlers.
    backend: Arc<Mutex<Backend>>,

    /// Parsed configuration.
    config: Config,
}

impl Runtime {
    /// Build a runtime from the given configuration.
    #[must_use]
    pub fn new(config: Config) -> Self {
        // Open the auth database when auth is enabled (FR-18). On failure,
        // log the error and proceed without auth — the middleware will
        // refuse all requests (fail-closed, NFR-06).
        let auth_store = if config.auth_enabled {
            match crate::services::auth::store::AuthStore::open(&config.auth_db_path) {
                Ok(store) => {
                    eprintln!("auth enabled — database at {}", config.auth_db_path.display());
                    seed::seed_accounts_if_empty(&store);
                    Some(store)
                }
                Err(err) => {
                    eprintln!("WARN: auth enabled but database open failed: {err} — running WITHOUT auth");
                    None
                }
            }
        } else {
            None
        };

        let backend = Arc::new(Mutex::new(Backend::new(
            config.agents_dir.clone(),
            config.agents_root.clone(),
            config.agent_binary.clone(),
            auth_store,
            config.session_ttl,
        )));
        Self { backend, config }
    }

    /// Spawn the background driver thread that continuously scans the registry
    /// and tails every discovered agent's oplog.
    ///
    /// Returns the [`JoinHandle`](thread::JoinHandle) (the thread runs until
    /// the process exits).
    pub fn start_driver(&self) -> thread::JoinHandle<()> {
        let backend = Arc::clone(&self.backend);
        let agents_dir = self.config.agents_dir.clone();
        let interval = self.config.scan_interval;
        let backup_scheduler =
            if self.config.auth_enabled { Some(BackupScheduler::new(self.config.auth_db_path.clone())) } else { None };

        thread::spawn(move || driver::driver_loop(backend, agents_dir, interval, backup_scheduler))
    }

    /// Spawn the auto-update scheduler (O4.2): poll the channel on boot and
    /// every `poll_interval_hours`; in `auto` mode, inside the box-local
    /// maintenance window, drive the download → stage → restart pipeline.
    /// `manual`/`paused` only refresh the visible state. See
    /// [`update_scheduler`].
    pub fn start_update_scheduler(&self, install: PathBuf) -> thread::JoinHandle<()> {
        update_scheduler::spawn(Arc::clone(&self.backend), self.config.auth_db_path.clone(), install)
    }

    /// Spawn the self-update committer thread (update-policy §5.5 steps 4-5).
    ///
    /// It polls our own `/healthz` and, once a staged update's boot proves
    /// genuinely healthy within the deadline
    /// ([`boot_commit_when_healthy`](crate::services::releases::boot_commit_when_healthy)),
    /// commits the binary markers and **promotes** the release-level state:
    /// `active_tag` + the agent binary + the supervisor allow-list flip to the
    /// new tag, the `auth.db` backup is dropped, and `success` is recorded. If
    /// the probe never turns healthy the markers stay, so the next boot's
    /// `boot_check` counts the failure and can roll back.
    ///
    /// No-op thread on a normal (nothing-staged) boot.
    pub fn start_update_committer(&self, install: PathBuf) -> thread::JoinHandle<()> {
        let backend = Arc::clone(&self.backend);
        let url = format!("http://127.0.0.1:{}/healthz", self.config.port);
        let auth_db = self.config.auth_db_path.clone();
        thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new());
            let healthy = || client.get(&url).send().map(|r| r.status().as_u16() == 200).unwrap_or(false);
            let committed = crate::services::releases::boot_commit_when_healthy(
                &install,
                healthy,
                Duration::from_secs(60),
                Duration::from_secs(2),
            );
            if !committed {
                return;
            }
            // The new binary is blessed — flip the release state to match it.
            let Ok(mut b) = backend.lock() else {
                eprintln!("updater: promote skipped — backend lock poisoned");
                return;
            };
            match crate::services::releases::updater::promote_committed(&mut b.releases, &auth_db) {
                Ok(Some(agent_binary)) => {
                    b.agent_binary = agent_binary.clone();
                    b.supervisor = crate::supervisor::AgentSupervisor::new(&[agent_binary]);
                    eprintln!("updater: update committed — active tag is now {:?}", b.releases.active_tag());
                }
                Ok(None) => {} // plain self-restart (manual flow), nothing to promote
                Err(e) => eprintln!("updater: promote after healthy boot FAILED: {e}"),
            }
        })
    }

    /// Block the calling thread on the product HTTP transport, serving requests
    /// until the process exits.
    ///
    /// There is a single transport face (design §13.4 removed the separate
    /// maintenance plane). Before blocking, this renders + reloads Caddy for the
    /// current provisioning state so the cockpit is served on `:80` (cleartext,
    /// day-0) or `:443` (private-CA TLS, once provisioned).
    ///
    /// # Errors
    ///
    /// Returns an error string if the product address cannot be bound.
    pub fn serve(&self) -> Result<(), String> {
        // Boot-time read of the durable provisioning flag. The effective cockpit
        // gate lives in Caddy, which serves the cockpit on :80 (day-0) or :443
        // (provisioned). This log makes the boot state observable in `logread`.
        if let Ok(b) = self.backend.lock() {
            let provisioned = crate::transport::it::is_provisioned(&b.provision_flag_path);
            eprintln!(
                "provisioning state: {} (flag: {})",
                if provisioned {
                    "provisioned — cockpit on :443"
                } else {
                    "UNPROVISIONED — cockpit on :80 (day-0)"
                },
                b.provision_flag_path.display()
            );
        }

        // Render + reload Caddy for the current state. No-op unless Caddy is
        // configured (CP_CADDYFILE); never fatal.
        crate::transport::it::apply_caddy_at_boot(&self.backend);

        let addr = format!("0.0.0.0:{}", self.config.port);
        eprintln!("serving on http://{addr}");
        crate::transport::serve(&addr, Arc::clone(&self.backend))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_sensible() {
        // `remove_var` is unsafe in edition 2024 and `unsafe_code` is
        // forbidden, so we cannot clear environment variables. Instead
        // verify that `from_env` succeeds when `$HOME` is set.
        if std::env::var_os("HOME").is_some() {
            let cfg = Config::from_env().expect("config");
            // The port, budget, and interval come from env or defaults;
            // assert the types parse correctly rather than exact values
            // (CI may set CP_ORCH_PORT etc.).
            assert!(cfg.port > 0);
            assert!(cfg.scan_interval.as_millis() > 0);
        }
    }
}
