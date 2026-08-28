//! The background **driver loop** — registry discovery, oplog tailing, and
//! freshness backstops, folded into the shared [`Backend`].
//!
//! Extracted from [`super`] (the runtime entry points) so the loop mechanics
//! live beside each other: the slow-cadence scan (discovery, tier-② mtime
//! backstop, tmp reap, auth backup) and the fast-cadence oplog tail
//! ([`TAIL_INTERVAL`]) that keeps the materialized view ~100 ms fresh.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use cp_wire::types::LifecycleState;
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::registry::Entry;

use crate::channel::Tailer;
use crate::registry::tee_reader::TeeReader;
use crate::registry::{Event, FleetScanner};
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

/// The per-agent bookkeeping the driver loop threads through its helpers:
/// oplog tailers, live stream readers, folder paths, and last-seen config
/// mtimes. Bundled so the fold/scan helpers take one `&mut` handle instead of
/// four parallel maps (keeps them within the argument budget). All four are
/// [`BTreeMap`]s so every iteration is deterministic.
#[derive(Default)]
struct DriverState {
    /// Per-agent oplog tailers, keyed by agent id.
    tailers: BTreeMap<String, Tailer>,
    /// Per-agent live stream-plane readers (tee.sock → hub republish).
    tee_readers: BTreeMap<String, TeeReader>,
    /// Per-agent working-directory paths, seeded from `Appeared` events.
    agent_folders: BTreeMap<String, PathBuf>,
    /// Per-agent last-seen `config.json` mtime, for change detection.
    config_mtimes: BTreeMap<String, SystemTime>,
}

/// The driver loop: registry scan → per-agent oplog tail → fold into shared
/// backend state. Runs forever on its own thread (hence `-> !`).
///
/// Two cadences, deliberately decoupled (design doc I12 / §8.1): the
/// **registry scan** + tier-② mtime backstop + tmp reap run once per slow
/// `interval`; the **oplog tail** — the live state-fold that feeds the view —
/// runs every [`TAIL_INTERVAL`] in a tight inner loop, so a freshly-appended
/// entry becomes visible in the view within ~100 ms rather than the (much
/// longer) registry-scan cadence.
pub(super) fn driver_loop(
    backend: &Arc<Mutex<Backend>>,
    agents_dir: PathBuf,
    interval: Duration,
    mut backup_scheduler: Option<BackupScheduler>,
) -> ! {
    let mut registry = FleetScanner::new(agents_dir);
    let mut ds = DriverState::default();

    loop {
        // ── Slow cadence: discovery + tier-② backstop + crash-orphan reap ──

        // 1. Registry scan — discover/lose agents, then sync liveness.
        if let Ok(events) = registry.scan() {
            process_registry_events(events, backend, &mut ds);
            sync_liveness(backend, &registry, &ds.agent_folders);
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
        check_config_mtimes(backend, &mut ds);

        // 3. Reap stale *.tmp registry writes (crash-orphans).
        let _reaped = registry.reap_tmp(crate::registry::DEFAULT_TMP_GRACE);

        // 4. Auth database backup (NFR-19/20) — rolling + daily snapshots.
        if let Some(scheduler) = backup_scheduler.as_mut()
            && let Ok(b) = backend.lock()
            && let Some(auth) = b.auth.as_ref()
        {
            scheduler.tick(auth);
        }

        // ── Fast cadence: fold every agent's oplog tail into the view ──
        //
        // Spin the tail on the tight inner interval until the next slow scan is
        // due, so durable deltas reach the view in ~TAIL_INTERVAL. A deadline
        // (not a precomputed tick count) avoids any integer division of the two
        // intervals.
        let scan_deadline = Instant::now().checked_add(interval);
        loop {
            tail_all_agents(backend, &mut ds.tailers);
            thread::sleep(TAIL_INTERVAL);
            if scan_deadline.is_none_or(|deadline| Instant::now() >= deadline) {
                break;
            }
        }
    }
}

/// Apply registry events: create/remove tailers + readers and update the
/// backend view. Consumes the owned event batch so each variant's payload is
/// moved out by value (no borrowed-enum match ergonomics).
fn process_registry_events(events: Vec<Event>, backend: &Arc<Mutex<Backend>>, ds: &mut DriverState) {
    for event in events {
        match event {
            Event::Appeared(entry) => handle_appeared(entry.as_ref(), backend, ds),
            Event::Disappeared(id) => {
                drop(ds.tailers.remove(&id));
                if let Some(reader) = ds.tee_readers.remove(&id) {
                    reader.stop();
                }
                drop(ds.agent_folders.remove(&id));
                let _: Option<SystemTime> = ds.config_mtimes.remove(&id);
                if let Ok(mut b) = backend.lock() {
                    drop(b.view_mut().remove(&id));
                    let _: Option<crate::liveness::Liveness> = b.liveness.remove(&id);
                }
            }
            Event::Stale(id, reason) => {
                // Store the stale verdict so fleet meta returns "disconnected",
                // and mark the agent dirty so the SSE invalidate fires promptly
                // (the frontend refetches agent meta within ~2s, not 15s).
                if let Ok(mut b) = backend.lock() {
                    let _: Option<crate::liveness::Liveness> = b.liveness.insert(id.clone(), reason);
                    b.mark_dirty(&id);
                }
            }
            Event::StatusChanged(..) => {}
        }
    }
}

/// Handle an `Appeared` agent: spawn its tailer + live stream reader, record
/// its folder, and seed its liveness as `Live`.
fn handle_appeared(entry: &Entry, backend: &Arc<Mutex<Backend>>, ds: &mut DriverState) {
    let oplog_dir = PathBuf::from(&entry.oplog_path);
    drop(ds.tailers.insert(entry.id.clone(), Tailer::new(oplog_dir)));
    let folder = PathBuf::from(&entry.folder);
    // Spawn the live stream reader for this agent's tee socket so its token
    // frames fan out through the hub to SSE subscribers.
    let reader = TeeReader::spawn(entry.id.clone(), &folder, Arc::clone(backend));
    if let Some(old) = ds.tee_readers.insert(entry.id.clone(), reader) {
        old.stop();
    }
    drop(ds.agent_folders.insert(entry.id.clone(), folder));
    if let Ok(mut b) = backend.lock() {
        let _: Option<crate::liveness::Liveness> = b.liveness.insert(entry.id.clone(), crate::liveness::Liveness::Live);
    }
}

/// Sync liveness for ALL known agents on every scan.
///
/// The registry emits `Event::Stale` on a Live→non-live transition but has NO
/// recovery event for Stale→Live. Without this sync, a briefly-stale agent that
/// recovers (heartbeat resumes, PID alive) would stay "disconnected" in the
/// backend forever. Recovered agents are marked dirty so the SSE invalidate
/// fires promptly.
fn sync_liveness(backend: &Arc<Mutex<Backend>>, registry: &FleetScanner, agent_folders: &BTreeMap<String, PathBuf>) {
    let Ok(mut b) = backend.lock() else {
        return;
    };
    for id in agent_folders.keys() {
        if let Some(live) = registry.liveness(id) {
            let prev = b.liveness.get(id).copied();
            let _: Option<crate::liveness::Liveness> = b.liveness.insert(id.clone(), live);
            // Agent recovered from stale — notify the frontend.
            if prev.is_some_and(|p| !p.is_live()) && live.is_live() {
                b.mark_dirty(id);
            }
        }
    }
}

/// Poll every agent's tailer and fold new entries into the shared backend.
///
/// When a `Lifecycle::Stopping` entry is seen, the agent is marked stale and
/// dirty immediately — this pushes "disconnected" to the frontend within one
/// SSE invalidate cycle (~ms) rather than waiting for the registry scan (~2s)
/// to notice the dead PID.
fn tail_all_agents(backend: &Arc<Mutex<Backend>>, tailers: &mut BTreeMap<String, Tailer>) {
    for (id, tailer) in tailers.iter_mut() {
        let Ok(entries) = tailer.poll() else {
            continue;
        };
        if entries.is_empty() {
            continue;
        }

        let has_stopping =
            entries.iter().any(|e| matches!(&e.kind, OpEntryKind::Lifecycle { state: LifecycleState::Stopping }));

        if let Ok(mut b) = backend.lock() {
            b.view_mut().apply_batch(id, &entries);
            if has_stopping {
                let _: Option<crate::liveness::Liveness> =
                    b.liveness.insert(id.clone(), crate::liveness::Liveness::StalePid);
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
fn check_config_mtimes(backend: &Arc<Mutex<Backend>>, ds: &mut DriverState) {
    // Disjoint field borrows: read the folders (shared) while recording new
    // mtimes (mutable) — two different maps, so the split borrow is sound and
    // needs no struct pattern (which would trip pattern_type_mismatch on the
    // `&mut DriverState`).
    let agent_folders = &ds.agent_folders;
    let config_mtimes = &mut ds.config_mtimes;
    for (id, folder) in agent_folders {
        let config_path = folder.join(CP_DIR).join(CONFIG_FILE);
        let Ok(current) = std::fs::metadata(&config_path).and_then(|m| m.modified()) else {
            continue;
        };

        let changed = config_mtimes.get(id).is_some_and(|prev| *prev != current);
        let _: Option<SystemTime> = config_mtimes.insert(id.clone(), current);

        if changed && let Ok(mut b) = backend.lock() {
            b.mark_dirty(id);
        }
    }
}
