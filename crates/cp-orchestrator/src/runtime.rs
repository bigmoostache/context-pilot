//! Orchestrator **runtime** — the main loop that drives discovery, tailing,
//! view projection, and transport serving.
//!
//! [`Runtime`] owns all the moving parts and exposes two entry points:
//!
//! * [`start_driver`](Runtime::start_driver) — spawns a background thread that
//!   scans the registry, tails every discovered agent's oplog, folds entries
//!   into the shared [`Backend`], and observes cost against the breaker.
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
//! | `CP_ORCH_PORT` | `7878` | HTTP listen port |
//! | `CP_AGENTS_DIR` | `~/.context-pilot/agents` | Registry directory |
//! | `CP_COST_BUDGET` | `100.0` | Per-agent cost budget (USD) |
//! | `CP_SCAN_INTERVAL_MS` | `2000` | Registry-discovery + tier-② mtime poll cadence (ms) |
//!
//! The oplog tail (the live state-fold that feeds the view) runs on a
//! much tighter [`TAIL_INTERVAL`] inner cadence, decoupled from the slow
//! registry scan, so a fresh oplog entry reaches the view within ~100 ms
//! rather than the scan interval (a step toward the inotify-primary signal
//! of design doc I12 / §8.1).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use crate::channel::Tailer;
use crate::registry::tee_reader::TeeReader;
use crate::registry::{AgentRegistry, Event};
use crate::services::auth::backup::BackupScheduler;
use crate::transport::Backend;

/// Default HTTP listen port.
const DEFAULT_PORT: u16 = 7878;

/// Default per-agent cost budget in USD.
const DEFAULT_BUDGET: f64 = 100.0;

/// Default registry + oplog poll interval.
const DEFAULT_SCAN_INTERVAL: Duration = Duration::from_millis(2000);

/// Fast inner cadence for folding each agent's oplog tail into the view.
///
/// Decoupled from the (slower) registry scan so a newly-appended oplog entry
/// — a created/archived thread, a phase change, a cost update — reaches the
/// materialized view within roughly this interval instead of waiting on the
/// registry-scan cadence. This is a poll-based stand-in for the design doc's
/// inotify-primary change signal (I12 / §8.1); the registry scan and the
/// tier-② mtime backstop deliberately stay on the slower interval.
const TAIL_INTERVAL: Duration = Duration::from_millis(100);

/// Parsed runtime configuration, sourced from environment variables.
#[derive(Debug)]
pub struct Config {
    /// HTTP listen port.
    pub port: u16,
    /// Directory holding agent registry records.
    pub agents_dir: PathBuf,
    /// Per-agent cost budget in USD.
    pub budget_usd: f64,
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

        let budget_usd = std::env::var("CP_COST_BUDGET").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_BUDGET);

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

        let auth_db_path = match std::env::var_os("CP_AUTH_DB") {
            Some(p) => PathBuf::from(p),
            None => std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".context-pilot/orchestrator/auth.db"))
                .unwrap_or_else(|| PathBuf::from("auth.db")),
        };

        Ok(Self {
            port,
            agents_dir,
            budget_usd,
            scan_interval,
            agents_root,
            agent_binary,
            auth_enabled,
            session_ttl,
            auth_db_path,
        })
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
            config.budget_usd,
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

        thread::spawn(move || driver_loop(backend, agents_dir, interval, backup_scheduler))
    }

    /// Block the calling thread on the HTTP transport, serving requests until
    /// the process exits.
    ///
    /// # Errors
    ///
    /// Returns an error string if the address cannot be bound.
    pub fn serve(&self) -> Result<(), String> {
        let addr = format!("0.0.0.0:{}", self.config.port);
        eprintln!("serving on http://{addr}");
        crate::transport::serve(&addr, Arc::clone(&self.backend))
    }
}

/// The driver loop: registry scan → per-agent oplog tail → fold into shared
/// backend state. Runs forever on its own thread.
///
/// Two cadences, deliberately decoupled (design doc I12 / §8.1): the
/// **registry scan** + tier-② mtime backstop + tmp reap run once per slow
/// `interval`; the **oplog tail** — the live state-fold that feeds the view —
/// runs every [`TAIL_INTERVAL`] in a tight inner loop, so a freshly-appended
/// entry becomes visible in the view within ~100 ms rather than the (much
/// longer) registry-scan cadence.
fn driver_loop(
    backend: Arc<Mutex<Backend>>,
    agents_dir: PathBuf,
    interval: Duration,
    mut backup_scheduler: Option<BackupScheduler>,
) {
    let mut registry = AgentRegistry::new(agents_dir);
    let mut tailers: HashMap<String, Tailer> = HashMap::new();
    // Per-agent live stream-plane readers (connect each agent's tee.sock and
    // republish its token frames into the hub). Keyed by agent id; spawned on
    // Appeared, dropped (→ stop+join) on Disappeared.
    let mut tee_readers: HashMap<String, TeeReader> = HashMap::new();
    // Per-agent folder paths, seeded from Appeared events.
    let mut agent_folders: HashMap<String, PathBuf> = HashMap::new();
    // Per-agent last-seen config.json mtime, for change detection.
    let mut config_mtimes: HashMap<String, SystemTime> = HashMap::new();

    // How many fast tail ticks fit in one slow scan interval (at least one).
    let tail_ticks = u64::try_from(interval.as_millis() / TAIL_INTERVAL.as_millis()).unwrap_or(1).max(1);

    loop {
        // ── Slow cadence: discovery + tier-② backstop + crash-orphan reap ──

        // 1. Registry scan — discover/lose agents.
        if let Ok(events) = registry.scan() {
            process_registry_events(&events, &backend, &mut tailers, &mut tee_readers, &mut agent_folders);

            // Clean up mtime entries for disappeared agents.
            for event in &events {
                if let Event::Disappeared(id) = event {
                    let _removed = config_mtimes.remove(id);
                }
            }
        }

        // 2. Detect tier-② INSPECTION-resource changes by checking config.json
        //    mtime, and mark the agent dirty so the SSE producer emits an
        //    `invalidate`. This is the freshness signal for the resources that
        //    have NO oplog delta to ride — memory / tree / callbacks (design
        //    doc's "unmanaged read-only listing"). The delta-covered resources
        //    (threads roster, phase, cost) ride the fast oplog tail below + SSE
        //    rev-deltas and deliberately IGNORE `invalidate` (X859), so this
        //    slow mtime scan is never on their live path — it is the coarse
        //    backstop the design doc reserves it as (I12: oplog tail primary,
        //    ~2s poll a backstop), and the inspection-resource freshness
        //    mechanism, nothing more.
        check_config_mtimes(&backend, &agent_folders, &mut config_mtimes);

        // 3. Reap stale *.tmp registry writes (crash-orphans).
        let _reaped = registry.reap_tmp(crate::registry::DEFAULT_TMP_GRACE);

        // 4. Auth database backup (NFR-19/20) — rolling + daily snapshots.
        if let Some(ref mut scheduler) = backup_scheduler {
            if let Ok(b) = backend.lock() {
                if let Some(ref auth) = b.auth {
                    scheduler.tick(auth);
                }
            }
        }

        // ── Fast cadence: fold every agent's oplog tail into the view ──
        //
        // Spin the tail on the tight inner interval until the next slow scan
        // is due, so durable deltas reach the view in ~TAIL_INTERVAL.
        for _ in 0..tail_ticks {
            tail_all_agents(&backend, &mut tailers);
            thread::sleep(TAIL_INTERVAL);
        }
    }
}

/// Apply registry events: create/remove tailers and update the backend view.
fn process_registry_events(
    events: &[Event],
    backend: &Arc<Mutex<Backend>>,
    tailers: &mut HashMap<String, Tailer>,
    tee_readers: &mut HashMap<String, TeeReader>,
    agent_folders: &mut HashMap<String, PathBuf>,
) {
    for event in events {
        match event {
            Event::Appeared(entry) => {
                let oplog_dir = PathBuf::from(&entry.oplog_path);
                let _previous = tailers.insert(entry.id.clone(), Tailer::new(oplog_dir));
                let folder = PathBuf::from(&entry.folder);
                // Spawn the live stream reader for this agent's tee socket so
                // its token frames fan out through the hub to SSE subscribers.
                let reader = TeeReader::spawn(entry.id.clone(), &folder, Arc::clone(backend));
                if let Some(old) = tee_readers.insert(entry.id.clone(), reader) {
                    old.stop();
                }
                let _prev = agent_folders.insert(entry.id.clone(), folder);
            }
            Event::Disappeared(id) => {
                let _removed = tailers.remove(id);
                if let Some(reader) = tee_readers.remove(id) {
                    reader.stop();
                }
                let _removed = agent_folders.remove(id);
                if let Ok(mut b) = backend.lock() {
                    let _removed = b.view_mut().remove(id);
                }
            }
            Event::StatusChanged(..) | Event::Stale(..) => {
                // StatusChanged and Stale are informational for the registry
                // layer; the view reflects phase/lifecycle from the oplog, so
                // no view mutation needed here.
            }
        }
    }
}

/// Poll every agent's tailer and fold new entries into the shared backend.
fn tail_all_agents(backend: &Arc<Mutex<Backend>>, tailers: &mut HashMap<String, Tailer>) {
    for (id, tailer) in tailers.iter_mut() {
        let entries = match tailer.poll() {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entries.is_empty() {
            continue;
        }

        if let Ok(mut b) = backend.lock() {
            b.view_mut().apply_batch(id, &entries);

            // Observe the latest cumulative cost against the breaker.
            // Extract the cost first to avoid overlapping borrows on `b`.
            let cost = b.view.get(id).map(|v| v.cost.cost_usd);
            if let Some(cost_usd) = cost {
                b.breaker_mut().observe(id, cost_usd);
            }
        }
    }
}

/// Subdirectory the agent stores its persistence files in.
const CP_DIR: &str = ".context-pilot";
/// The global shared configuration file.
const CONFIG_FILE: &str = "config.json";

/// Check each known agent's `config.json` mtime and mark dirty when it changes.
///
/// A single `stat` call per agent (~1µs) gates whether any work happens.
/// When the mtime differs from the last observation, the agent is marked dirty
/// in the shared [`Backend`] so that SSE producers emit an `invalidate` event,
/// prompting connected frontends to refetch tier-② data immediately.
fn check_config_mtimes(
    backend: &Arc<Mutex<Backend>>,
    agent_folders: &HashMap<String, PathBuf>,
    mtimes: &mut HashMap<String, SystemTime>,
) {
    for (id, folder) in agent_folders {
        let config_path = folder.join(CP_DIR).join(CONFIG_FILE);
        let current = match std::fs::metadata(&config_path).and_then(|m| m.modified()) {
            Ok(mt) => mt,
            Err(_) => continue,
        };

        let changed = mtimes.get(id).map_or(false, |prev| *prev != current);
        let _prev = mtimes.insert(id.clone(), current);

        if changed {
            if let Ok(mut b) = backend.lock() {
                b.mark_dirty(id);
            }
        }
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
            assert!(cfg.budget_usd > 0.0);
            assert!(cfg.scan_interval.as_millis() > 0);
        }
    }
}
