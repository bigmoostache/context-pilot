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
//! | `CP_ORCH_BIND` | `127.0.0.1` | Listen address — loopback, Caddy fronts the LAN |
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

/// Default listen address: **loopback only**.
///
/// The cockpit's only LAN-facing surface is Caddy, which terminates TLS and
/// proxies to `127.0.0.1:7878` (see [`crate::transport::it::caddy`]). Binding
/// every interface would publish the backend itself on the LAN in cleartext,
/// letting anyone bypass the `:80`→`:443` redirect the provisioned box relies
/// on — while the auth model (bearer token, CORS) assumes an encrypted
/// transport. Override with `CP_ORCH_BIND` (e.g. `0.0.0.0` for a dev box whose
/// UI is opened from another machine).
const DEFAULT_BIND: &str = "127.0.0.1";

/// Default registry + oplog poll interval.
const DEFAULT_SCAN_INTERVAL: Duration = Duration::from_secs(2);

/// Parsed runtime configuration, sourced from environment variables.
#[derive(Debug)]
pub struct Config {
    /// Product cockpit HTTP listen port.
    pub port: u16,
    /// Address the cockpit binds (`CP_ORCH_BIND`, default `127.0.0.1` —
    /// loopback, so every LAN request arrives through Caddy).
    pub bind: String,
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
    /// Path to the auth `SQLite` database (`CP_AUTH_DB`, default
    /// `~/.context-pilot/orchestrator/auth.db`). Orchestrator-level storage,
    /// not inside `agents_dir` (D7/Q9).
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

        // Loopback unless explicitly widened: the appliance's LAN surface is
        // Caddy, never the backend socket.
        let bind = std::env::var("CP_ORCH_BIND")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BIND.to_owned());

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
        let agents_root = std::env::var_os("CP_AGENTS_ROOT").map_or_else(
            || std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), |h| PathBuf::from(h).join("code")),
            PathBuf::from,
        );

        // The `cp` TUI binary the supervisor spawns. Default to the release
        // build under the current working directory; override with an absolute
        // path in deployment.
        let agent_binary = std::env::var_os("CP_AGENT_BINARY").map_or_else(
            || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("target/release/tui"),
            PathBuf::from,
        );

        // Auth configuration (§8 of design doc).
        let auth_enabled =
            std::env::var("CP_AUTH_ENABLED").ok().is_some_and(|s| s.eq_ignore_ascii_case("true") || s == "1");

        let session_ttl = std::env::var("CP_SESSION_TTL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map_or(Duration::from_hours(720), Duration::from_secs); // 30 days

        let auth_db_path = crate::services::auth::db::AuthStore::default_db_path();

        Ok(Self {
            port,
            bind,
            agents_dir,
            scan_interval,
            agents_root,
            agent_binary,
            auth_enabled,
            session_ttl,
            auth_db_path,
        })
    }

    /// The `host:port` string handed to the HTTP acceptor.
    ///
    /// IPv6 literals are bracketed (`[::1]:7878`) so `to_socket_addrs` parses
    /// them; an already-bracketed value is left alone.
    #[must_use]
    pub fn listen_addr(&self) -> String {
        let host = self.bind.as_str();
        if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]:{}", self.port)
        } else {
            format!("{host}:{}", self.port)
        }
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
            match crate::services::auth::db::AuthStore::open(&config.auth_db_path) {
                Ok(store) => {
                    crate::oerr!("auth enabled — database at {}", config.auth_db_path.display());
                    seed::seed_accounts_if_empty(&store);
                    Some(store)
                }
                Err(err) => {
                    crate::oerr!("WARN: auth enabled but database open failed: {err} — running WITHOUT auth");
                    None
                }
            }
        } else {
            None
        };

        let backend = Arc::new(Mutex::new(Backend::new(
            crate::transport::Paths {
                agents_dir: config.agents_dir.clone(),
                agents_root: config.agents_root.clone(),
                agent_binary: config.agent_binary.clone(),
            },
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
    #[must_use]
    pub fn start_driver(&self) -> thread::JoinHandle<()> {
        let backend = Arc::clone(&self.backend);
        let agents_dir = self.config.agents_dir.clone();
        let interval = self.config.scan_interval;
        let backup_scheduler = self.config.auth_enabled.then(|| BackupScheduler::new(self.config.auth_db_path.clone()));

        thread::spawn(move || driver::driver_loop(&backend, agents_dir, interval, backup_scheduler))
    }

    /// Spawn the auto-update scheduler (O4.2): poll the channel on boot and
    /// every `poll_interval_hours`; in `auto` mode, inside the box-local
    /// maintenance window, drive the download → stage → restart pipeline.
    /// `manual`/`paused` only refresh the visible state. See
    /// [`update_scheduler`].
    #[must_use]
    pub fn start_update_scheduler(&self, install: PathBuf) -> thread::JoinHandle<()> {
        update_scheduler::spawn(Arc::clone(&self.backend), self.config.auth_db_path.clone(), install)
    }

    /// Spawn the Claude OAuth refresh sweeper: a standalone thread that keeps the
    /// active token AND every stored account fresh, refreshing any that fall
    /// within an hour of expiry. Needs no backend state (OAuth lives on disk /
    /// Keychain), so it takes nothing and holds no locks. See
    /// [`crate::transport::rest::spawn_oauth_refresh`].
    #[must_use]
    pub fn start_oauth_sweeper() -> thread::JoinHandle<()> {
        crate::transport::rest::spawn_oauth_refresh()
    }

    /// Spawn the SMS ingester: sweep the modem's message storage into the local
    /// archive, then free the slot.
    ///
    /// It runs on every box, not only 5G ones — the tick's first act is the
    /// same sysfs modem probe the cockpit gates on, so on a box with no module
    /// the thread costs one directory read every 30 s and nothing else. That is
    /// cheaper than threading the hardware answer through boot, and it stays
    /// correct if a module is ever fitted without a reflash. See
    /// [`sms::poll`](crate::transport::it::network::sms::poll).
    #[must_use]
    pub fn start_sms_poller(&self) -> thread::JoinHandle<()> {
        crate::transport::it::network::sms::poll::spawn(Arc::clone(&self.backend))
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
    #[must_use]
    pub fn start_update_committer(&self, install: PathBuf) -> thread::JoinHandle<()> {
        let backend = Arc::clone(&self.backend);
        let url = format!("http://127.0.0.1:{}/healthz", self.config.port);
        let auth_db = self.config.auth_db_path.clone();
        thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new());
            let healthy = || client.get(&url).send().is_ok_and(|r| r.status().as_u16() == 200);
            let committed = crate::services::releases::boot_commit_when_healthy(
                &install,
                healthy,
                Duration::from_mins(1),
                Duration::from_secs(2),
            );
            if !committed {
                return;
            }
            // The new binary is blessed — flip the release state to match it.
            let Ok(mut b) = backend.lock() else {
                crate::oerr!("updater: promote skipped \u{2014} backend lock poisoned");
                return;
            };
            match crate::services::releases::updater::apply::promote_committed(&mut b.releases, &auth_db) {
                Ok(Some(agent_binary)) => {
                    b.agent_binary.clone_from(&agent_binary);
                    b.supervisor = crate::supervisor::ProcManager::new(&[agent_binary]);
                    crate::oerr!("updater: update committed — active tag is now {:?}", b.releases.active_tag());
                }
                Ok(None) => {} // plain self-restart (manual flow), nothing to promote
                Err(e) => crate::oerr!("updater: promote after healthy boot FAILED: {e}"),
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
            crate::oerr!(
                "provisioning state: {} (flag: {})",
                if provisioned {
                    "provisioned \u{2014} cockpit on :443"
                } else {
                    "UNPROVISIONED \u{2014} cockpit on :80 (day-0)"
                },
                b.provision_flag_path.display()
            );
        }

        // Render + reload Caddy for the current state. No-op unless Caddy is
        // configured (CP_CADDYFILE); never fatal.
        crate::transport::it::apply_caddy_at_boot(&self.backend);

        // Re-apply the persisted uplink/AP configuration: it is written
        // atomically and durably, so it must survive a power cut and be back in
        // force before the cockpit serves. No-op unless the network gates are
        // set (CP_NMCLI_BIN); never fatal — a box whose modem is missing, whose
        // SIM is absent or whose radio is rfkilled must still boot into a
        // reachable cockpit, so a failure here is a journal line, not a dead
        // appliance.
        crate::transport::it::apply_network_at_boot(&self.backend);

        let addr = self.config.listen_addr();
        crate::oerr!("serving on http://{addr}");
        crate::transport::serve(&addr, &self.backend)
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

    #[test]
    fn default_bind_is_loopback() {
        // Not cosmetic: the backend speaks cleartext HTTP and its auth model
        // assumes an encrypted transport, so its socket must never face the
        // LAN — Caddy does (`:80` day-0, `:443` provisioned).
        let ip: std::net::IpAddr = DEFAULT_BIND.parse().expect("default bind is an IP literal");
        assert!(ip.is_loopback(), "the cockpit backend must bind loopback; Caddy fronts the LAN");
    }

    #[test]
    fn listen_addr_brackets_ipv6_literals() {
        if std::env::var_os("HOME").is_none() {
            return;
        }
        let mut cfg = Config::from_env().expect("config");
        cfg.port = 7878;

        cfg.bind = "127.0.0.1".to_owned();
        assert_eq!(cfg.listen_addr(), "127.0.0.1:7878");

        cfg.bind = "::1".to_owned();
        assert_eq!(cfg.listen_addr(), "[::1]:7878");

        // An operator who brackets it themselves must not get `[[::1]]`.
        cfg.bind = "[::1]".to_owned();
        assert_eq!(cfg.listen_addr(), "[::1]:7878");
    }
}
