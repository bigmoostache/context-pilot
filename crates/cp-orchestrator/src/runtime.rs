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
//! | `CP_SCAN_INTERVAL_MS` | `2000` | Registry + oplog poll cadence (ms) |

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::channel::Tailer;
use crate::registry::{AgentRegistry, Event};
use crate::transport::Backend;

/// Default HTTP listen port.
const DEFAULT_PORT: u16 = 7878;

/// Default per-agent cost budget in USD.
const DEFAULT_BUDGET: f64 = 100.0;

/// Default registry + oplog poll interval.
const DEFAULT_SCAN_INTERVAL: Duration = Duration::from_millis(2000);

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
}

impl Config {
    /// Read configuration from environment variables, falling back to defaults.
    ///
    /// # Errors
    ///
    /// Returns a message if `CP_AGENTS_DIR` is absent **and** `$HOME` is unset
    /// (so the default directory cannot be derived).
    pub fn from_env() -> Result<Self, String> {
        let port = std::env::var("CP_ORCH_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PORT);

        let agents_dir = match std::env::var_os("CP_AGENTS_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => crate::registry::default_agents_dir()
                .map_err(|e| format!("cannot derive agents directory: {e}"))?,
        };

        let budget_usd = std::env::var("CP_COST_BUDGET")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_BUDGET);

        let scan_interval = std::env::var("CP_SCAN_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map_or(DEFAULT_SCAN_INTERVAL, Duration::from_millis);

        Ok(Self { port, agents_dir, budget_usd, scan_interval })
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
        let backend = Arc::new(Mutex::new(Backend::new(
            config.agents_dir.clone(),
            config.budget_usd,
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

        thread::spawn(move || driver_loop(backend, agents_dir, interval))
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
fn driver_loop(backend: Arc<Mutex<Backend>>, agents_dir: PathBuf, interval: Duration) {
    let mut registry = AgentRegistry::new(agents_dir);
    let mut tailers: HashMap<String, Tailer> = HashMap::new();

    loop {
        // 1. Registry scan — discover/lose agents.
        if let Ok(events) = registry.scan() {
            process_registry_events(&events, &backend, &mut tailers);
        }

        // 2. Tail every known agent's oplog and fold into the view.
        tail_all_agents(&backend, &mut tailers);

        // 3. Reap stale *.tmp registry writes (crash-orphans).
        let _reaped = registry.reap_tmp(crate::registry::DEFAULT_TMP_GRACE);

        thread::sleep(interval);
    }
}

/// Apply registry events: create/remove tailers and update the backend view.
fn process_registry_events(
    events: &[Event],
    backend: &Arc<Mutex<Backend>>,
    tailers: &mut HashMap<String, Tailer>,
) {
    for event in events {
        match event {
            Event::Appeared(entry) => {
                let oplog_dir = PathBuf::from(&entry.oplog_path);
                let _previous = tailers.insert(entry.id.clone(), Tailer::new(oplog_dir));
            }
            Event::Disappeared(id) => {
                let _removed = tailers.remove(id);
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
