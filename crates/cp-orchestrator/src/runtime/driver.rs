//! The background **driver loop** — registry discovery, oplog tailing, and
//! freshness backstops, folded into the shared [`Backend`].
//!
//! Extracted from [`super`] (the runtime entry points) so the loop mechanics
//! live beside each other: the slow-cadence scan (discovery, tier-② mtime
//! backstop, tmp reap, auth backup) and the fast-cadence oplog tail
//! ([`TAIL_INTERVAL`]) that keeps the materialized view ~100 ms fresh.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use cp_wire::types::LifecycleState;
use cp_wire::types::oplog::OpEntryKind;

use crate::channel::Tailer;
use crate::registry::tee_reader::TeeReader;
use crate::registry::{AgentRegistry, Event};
use crate::services::auth::backup::BackupScheduler;
use crate::transport::Backend;

/// Fast inner cadence for folding each agent's oplog tail into the view.
///
/// Decoupled from the (slower) registry scan so a newly-appended oplog entry
/// — a created/archived thread, a phase change, a cost update — reaches the
/// materialized view within roughly this interval instead of waiting on the
/// registry-scan cadence. This is a poll-based stand-in for the design doc's
/// inotify-primary change signal (I12 / §8.1); the registry scan and the
/// tier-② mtime backstop deliberately stay on the slower interval.
pub(super) const TAIL_INTERVAL: Duration = Duration::from_millis(100);

/// The driver loop: registry scan → per-agent oplog tail → fold into shared
/// backend state. Runs forever on its own thread.
///
/// Two cadences, deliberately decoupled (design doc I12 / §8.1): the
/// **registry scan** + tier-② mtime backstop + tmp reap run once per slow
/// `interval`; the **oplog tail** — the live state-fold that feeds the view —
/// runs every [`TAIL_INTERVAL`] in a tight inner loop, so a freshly-appended
/// entry becomes visible in the view within ~100 ms rather than the (much
/// longer) registry-scan cadence.
pub(super) fn driver_loop(
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

            // Sync liveness for ALL known agents on every scan. The registry
            // emits `Event::Stale` on a Live→non-live transition but has NO
            // recovery event for Stale→Live. Without this sync, a briefly-stale
            // agent that recovers (heartbeat resumes, PID alive) would stay
            // "disconnected" in the Backend forever. Mark recovered agents dirty
            // so the SSE invalidate fires promptly.
            if let Ok(mut b) = backend.lock() {
                for id in agent_folders.keys() {
                    if let Some(live) = registry.liveness(id) {
                        let prev = b.liveness.get(id).copied();
                        let _prev = b.liveness.insert(id.clone(), live);
                        // Agent recovered from stale — notify frontend.
                        if prev.is_some_and(|p| !p.is_live()) && live.is_live() {
                            b.mark_dirty(id);
                        }
                    }
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
                if let Ok(mut b) = backend.lock() {
                    let _prev = b.liveness.insert(entry.id.clone(), crate::liveness::Liveness::Live);
                }
            }
            Event::Disappeared(id) => {
                let _removed = tailers.remove(id);
                if let Some(reader) = tee_readers.remove(id) {
                    reader.stop();
                }
                let _removed = agent_folders.remove(id);
                if let Ok(mut b) = backend.lock() {
                    let _removed = b.view_mut().remove(id);
                    let _removed = b.liveness.remove(id);
                }
            }
            Event::Stale(id, reason) => {
                // Store the stale verdict so fleet meta returns "disconnected",
                // and mark the agent dirty so the SSE invalidate fires promptly
                // (the frontend refetches agent meta within ~2s, not 15s).
                if let Ok(mut b) = backend.lock() {
                    let _prev = b.liveness.insert(id.clone(), *reason);
                    b.mark_dirty(id);
                }
            }
            Event::StatusChanged(..) => {}
        }
    }
}

/// Poll every agent's tailer and fold new entries into the shared backend.
///
/// When a `Lifecycle::Stopping` entry is seen, the agent is marked stale and
/// dirty immediately — this pushes "disconnected" to the frontend within one
/// SSE invalidate cycle (~ms) rather than waiting for the registry scan (~2s)
/// to notice the dead PID.
fn tail_all_agents(backend: &Arc<Mutex<Backend>>, tailers: &mut HashMap<String, Tailer>) {
    for (id, tailer) in tailers.iter_mut() {
        let entries = match tailer.poll() {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entries.is_empty() {
            continue;
        }

        let has_stopping =
            entries.iter().any(|e| matches!(&e.kind, OpEntryKind::Lifecycle { state: LifecycleState::Stopping }));

        if let Ok(mut b) = backend.lock() {
            b.view_mut().apply_batch(id, &entries);
            if has_stopping {
                let _prev = b.liveness.insert(id.clone(), crate::liveness::Liveness::StalePid);
                b.mark_dirty(id);
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
