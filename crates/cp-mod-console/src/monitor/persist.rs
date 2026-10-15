//! Monitor persistence across TUI reloads.
//!
//! Consoles survive a reload (the console server keeps the process, the TUI
//! reconnects and refills the ring buffer from the log file), so monitors must
//! too. Each record is saved with the ring-buffer cursor its watcher has
//! consumed and the agent it notifies. Because a reconnected buffer is rebuilt
//! from the whole log file, ring offsets equal log-file offsets on both sides
//! of the reload: the restored watcher resumes exactly where it stopped, and
//! output produced during the reload is still delivered.

use std::sync::{Arc, Mutex, PoisonError};

use cp_base::state::runtime::State;
use cp_base::state::watchers::WatcherRegistry;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::spec::Record;
use super::watcher::LiveMonitor;
use crate::types::ConsoleState;

/// On-disk shape of one monitor.
#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    /// Panel record (params, ids, status, event count).
    record: Record,
    /// Ring-buffer offset already consumed by the watcher.
    cursor: u64,
    /// Agent the notifications go to.
    agent: Option<String>,
}

/// Snapshot every tracked monitor (alive and recently ended).
#[must_use]
pub fn save(cs: &ConsoleState) -> Value {
    let saved: Vec<Saved> = cs
        .monitors
        .iter()
        .map(|m| {
            let record = m.lock().unwrap_or_else(PoisonError::into_inner).clone();
            Saved { cursor: record.cursor, agent: record.agent.clone(), record }
        })
        .collect();
    serde_json::to_value(saved).unwrap_or(Value::Null)
}

/// Restore monitors saved by [`save`] and re-register a watcher for each alive
/// one. `monitors` is the saved array, `next_id` the saved id counter.
///
/// Needs the spine's `WatcherRegistry`: every module's `init_state` runs before
/// any `load_module_data`, so it exists by the time the console loads.
pub fn restore(monitors: Option<&Value>, next_id: Option<u64>, state: &mut State) {
    let saved: Vec<Saved> = monitors.and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let cs = ConsoleState::get_mut(state);
    if let Some(n) = next_id.and_then(|n| usize::try_from(n).ok()) {
        cs.next_monitor_id = cs.next_monitor_id.max(n);
    }
    let mut live: Vec<LiveMonitor> = Vec::new();
    for Saved { mut record, cursor, agent } in saved {
        record.cursor = cursor;
        record.agent.clone_from(&agent);
        let alive = record.is_alive();
        let shared = Arc::new(Mutex::new(record));
        if alive {
            live.push(LiveMonitor::new(Arc::clone(&shared), cursor, agent));
        }
        cs.monitors.push(shared);
    }
    cs.prune_monitors();
    let registry = WatcherRegistry::get_mut(state);
    for watcher in live {
        registry.register(Box::new(watcher));
    }
}
