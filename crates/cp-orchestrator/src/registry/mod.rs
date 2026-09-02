//! [`FleetScanner`] — fleet discovery via directory scan-and-diff
//! (design doc §10, roadmap P5-T1).
//!
//! An agent advertises itself by atomically writing
//! `~/.context-pilot/agents/<id>.json` at boot (`cp-mod-bridge`'s registry
//! writer) and rewriting `<folder>/heartbeat` at a fixed cadence
//! (`cp_wire::heartbeat`). [`FleetScanner`] reads that directory, derives a
//! [`Liveness`] verdict per record (see [`liveness`]), and diffs each
//! pass against the last to emit fleet-change [`Event`]s.
//!
//! # Scan-and-diff, not a kernel watch
//!
//! Discovery is **poll-based** ([`FleetScanner::scan`]): each pass reads the
//! directory, parses every record, computes each verdict, and diffs the result
//! against the previous pass. Agents appear and disappear rarely (boot /
//! shutdown), so a directory poll at a modest cadence meets the "within one
//! cadence" latency target without the per-file watch budget that the *oplog*
//! tail (a high-frequency stream, design doc I12) genuinely needs. Keeping the
//! core a pure scan+diff also makes it testable against real files and pids with
//! no timing flakiness — the live driver is a thin loop that calls
//! [`scan`](FleetScanner::scan) and [`reap_tmp`](FleetScanner::reap_tmp) each
//! tick.
//!
//! A registry write is `tmp → fsync → rename`, so a crashed writer can leave a
//! `*.tmp` orphan. [`reap_tmp`](FleetScanner::reap_tmp) deletes those once they
//! are older than a grace window, exactly as the body store reaps crash-orphan
//! bodies (design doc GAP 3) — the grace must exceed the longest write window so
//! an in-flight `*.tmp` about to be renamed is never deleted out from under it.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cp_wire::heartbeat::{DEFAULT_MAX_AGE, Heartbeat};
use cp_wire::types::registry::{AgentStatus, Entry};

pub mod channel;
pub mod liveness;
pub mod tailer;
pub mod tee_reader;

use self::liveness::{Liveness, verdict};

/// File-name suffix of a published registry record.
const RECORD_SUFFIX: &str = ".json";

/// File-name suffix of an in-progress (pre-rename) registry write.
const TMP_SUFFIX: &str = ".tmp";

/// Default grace before a leftover `*.tmp` registry write is reaped.
///
/// Must exceed the longest possible `tmp → fsync → rename` window so a write
/// in flight right now is never mistaken for a crash-orphan and deleted. A
/// single small-file write + rename is sub-millisecond; 60 s is vastly larger,
/// so only genuine crash-orphans are ever collected.
pub const DEFAULT_TMP_GRACE: Duration = Duration::from_mins(1);

/// Default grace before an orphaned per-agent sync dir is reaped (T713).
///
/// A `~/.context-pilot/sync/<id>/` dir whose `<id>` no longer has any registry
/// record belongs to a deleted realm. The 7-day grace is deliberately generous:
/// the sync dir holds the agent's durable oplog, so an over-eager reap would
/// destroy history if a realm's record were ever transiently missing. Only a
/// dir untouched for a full week AND with no registry record is collected.
pub const DEFAULT_SYNC_ORPHAN_GRACE: Duration = Duration::from_hours(168); // 7 days

/// A change in the fleet observed between two [`scan`](FleetScanner::scan)es.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A record with a previously-unseen id was discovered. Carries the full
    /// record (boxed — it is far larger than the other variants' payloads, so
    /// boxing keeps `Event` small); the agent's liveness is queryable via
    /// [`FleetScanner::liveness`].
    Appeared(Box<Entry>),

    /// A previously-known record is no longer present (graceful shutdown
    /// removed it, or it was reaped).
    Disappeared(String),

    /// A known agent's registry `status` field changed (e.g. `Starting` →
    /// `Running`).
    StatusChanged(String, AgentStatus),

    /// A known agent's liveness transitioned from [`Live`](Liveness::Live) to a
    /// non-live verdict — it died, hung, or its pid was recycled.
    Stale(String, Liveness),
}

/// One agent's last-observed state, retained between scans to compute diffs.
#[derive(Clone, Debug)]
struct Snapshot {
    /// The most recently parsed registry record.
    entry: Entry,

    /// The most recently derived liveness verdict.
    liveness: Liveness,
}

/// Watches an agents directory and reports fleet membership and liveness.
///
/// Construct with [`new`](FleetScanner::new), then call
/// [`scan`](FleetScanner::scan) on a cadence to drive [`Event`]s and
/// [`reap_tmp`](FleetScanner::reap_tmp) to clear crash-orphan writes.
#[derive(Debug)]
pub struct FleetScanner {
    /// Directory holding `<id>.json` records (and transient `*.tmp` writes).
    dir: PathBuf,

    /// Heartbeat freshness window applied by the liveness verdict.
    max_age: Duration,

    /// Last-observed state per agent id, for diffing the next scan.
    known: HashMap<String, Snapshot>,
}

impl FleetScanner {
    /// Watch `dir` with the default heartbeat freshness window
    /// ([`DEFAULT_MAX_AGE`]).
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self::with_max_age(dir, DEFAULT_MAX_AGE)
    }

    /// Watch `dir` with an explicit heartbeat freshness window (tests use a
    /// tiny window to force staleness without sleeping).
    #[must_use]
    pub fn with_max_age(dir: PathBuf, max_age: Duration) -> Self {
        Self { dir, max_age, known: HashMap::new() }
    }

    /// The liveness verdict last derived for `id`, or `None` if `id` is not
    /// currently known.
    #[must_use]
    pub fn liveness(&self, id: &str) -> Option<Liveness> {
        self.known.get(id).map(|snap| snap.liveness)
    }

    /// The number of agents currently known (regardless of liveness).
    #[must_use]
    pub fn len(&self) -> usize {
        self.known.len()
    }

    /// Whether no agents are currently known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// Scan the directory once, updating internal state and returning the
    /// [`Event`]s that describe how the fleet changed since the previous scan.
    ///
    /// Records that cannot be read or parsed are skipped (a half-written or
    /// foreign file is not a fatal condition for the whole fleet). The order of
    /// emitted events is unspecified.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] only if the directory itself cannot be listed; a
    /// missing directory yields an empty scan (no agents yet), not an error.
    pub fn scan(&mut self) -> io::Result<Vec<Event>> {
        let now_ms = now_ms();
        let mut fresh: HashMap<String, Snapshot> = HashMap::new();

        for entry in read_records(&self.dir)? {
            let liveness = verdict(&entry, read_heartbeat(&entry).as_ref(), now_ms, self.max_age);
            // Ids are unique (one record per id), so no prior value is expected;
            // bind-and-discard satisfies the forbid-unused-results lint.
            let _previous = fresh.insert(entry.id.clone(), Snapshot { entry, liveness });
        }

        let events = self.diff(&fresh);
        self.known = fresh;
        Ok(events)
    }

    /// Compute the events between the current `known` state and a freshly
    /// scanned `fresh` state, without mutating either.
    fn diff(&self, fresh: &HashMap<String, Snapshot>) -> Vec<Event> {
        let mut events = Vec::new();

        // Disappearances: known ids absent from the fresh scan. Collect the keys
        // into a Vec first — iterating the HashMap directly trips
        // clippy::iter_over_hash_type (non-deterministic order); order is
        // irrelevant to a diff, but the collected Vec satisfies the lint.
        let known_ids: Vec<&String> = self.known.keys().collect();
        for id in known_ids {
            if !fresh.contains_key(id) {
                events.push(Event::Disappeared(id.clone()));
            }
        }

        let fresh_pairs: Vec<(&String, &Snapshot)> = fresh.iter().collect();
        for (id, snap) in fresh_pairs {
            match self.known.get(id) {
                None => events.push(Event::Appeared(Box::new(snap.entry.clone()))),
                Some(prev) => {
                    if prev.entry.status != snap.entry.status {
                        events.push(Event::StatusChanged(id.clone(), snap.entry.status));
                    }
                    // A transition out of Live is the actionable "went stale"
                    // signal; staleness present at first sight rides Appeared.
                    if prev.liveness.is_live() && !snap.liveness.is_live() {
                        events.push(Event::Stale(id.clone(), snap.liveness));
                    }
                }
            }
        }
        events
    }

    /// Delete `*.tmp` registry writes older than `grace` and return how many
    /// were removed (design doc GAP 3, applied to registry writes).
    ///
    /// A `*.tmp` younger than `grace` is an in-flight write about to be renamed
    /// and is left untouched; only provable crash-orphans are collected. Use
    /// [`DEFAULT_TMP_GRACE`] unless a measured write window justifies otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the directory cannot be listed or a removal
    /// fails. A file whose age cannot be determined is conservatively kept.
    pub fn reap_tmp(&self, grace: Duration) -> io::Result<u64> {
        let read_dir = match fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };

        let mut removed: u64 = 0;
        for raw in read_dir {
            let entry = raw?;
            let path = entry.path();
            if !path.to_string_lossy().ends_with(TMP_SUFFIX) {
                continue;
            }
            if let Some(age) = file_age(&path)
                && age > grace
            {
                fs::remove_file(&path)?;
                removed = removed.wrapping_add(1);
            }
        }
        Ok(removed)
    }

    /// Delete per-agent sync dirs under `sync_root` whose id has no current
    /// registry record and which are older than `grace`, returning how many were
    /// removed (T713 — orphans left behind by deleted realms).
    ///
    /// A subdirectory is collected only when BOTH hold: its name is NOT a
    /// currently-known agent id (a realm merely stale-but-registered keeps its
    /// record, so its dir is preserved for recovery), and it has been untouched
    /// for longer than `grace`. Because the sync dir holds the agent's durable
    /// oplog, the guard is deliberately conservative — use
    /// [`DEFAULT_SYNC_ORPHAN_GRACE`].
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if `sync_root` cannot be listed or a removal fails;
    /// a missing `sync_root` yields `Ok(0)`. A dir whose age cannot be read is
    /// conservatively kept.
    pub fn reap_orphan_sync_dirs(&self, sync_root: &Path, grace: Duration) -> io::Result<u64> {
        let read_dir = match fs::read_dir(sync_root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };

        let mut removed: u64 = 0;
        for raw in read_dir {
            let entry = raw?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(id) = name.to_str() else { continue };
            if self.known.contains_key(id) {
                continue; // a registered agent (live or merely stale) — keep it.
            }
            let path = entry.path();
            if let Some(age) = file_age(&path)
                && age > grace
            {
                fs::remove_dir_all(&path)?;
                removed = removed.wrapping_add(1);
            }
        }
        Ok(removed)
    }
}

/// Read and decode the heartbeat at the record's advertised path, or `None` if
/// it is absent, the wrong length, torn (CRC), or otherwise undecodable — every
/// such case means "no trustworthy beat", which the verdict treats as stale.
fn read_heartbeat(entry: &Entry) -> Option<Heartbeat> {
    let bytes = fs::read(&entry.heartbeat_path).ok()?;
    Heartbeat::decode(&bytes).ok()
}

/// Parse every `<id>.json` record in `dir`, skipping unreadable/unparseable
/// files and the transient `*.tmp` writes. A missing directory yields an empty
/// list (no agents yet), not an error.
fn read_records(dir: &Path) -> io::Result<Vec<Entry>> {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut records = Vec::new();
    for raw in read_dir {
        let entry = raw?;
        let path = entry.path();
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else { continue };
        if !name.ends_with(RECORD_SUFFIX) || name.ends_with(TMP_SUFFIX) {
            continue;
        }
        if let Ok(bytes) = fs::read(&path)
            && let Ok(record) = serde_json::from_slice::<Entry>(&bytes)
        {
            records.push(record);
        }
    }
    Ok(records)
}

/// The age of the file at `path`, or `None` if its modification time cannot be
/// read (so a caller conservatively keeps it).
fn file_age(path: &Path) -> Option<Duration> {
    fs::metadata(path).ok()?.modified().ok()?.elapsed().ok()
}

/// The default agents directory the fleet advertises into:
/// `$HOME/.context-pilot/agents`.
///
/// Mirrors the agent-side `cp-mod-bridge` registry writer so the backend reads
/// exactly where agents write.
///
/// # Errors
///
/// Returns [`io::Error`] if `$HOME` is unset.
pub fn default_agents_dir() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "$HOME is not set"))?;
    Ok(Path::new(&home).join(".context-pilot").join("agents"))
}

/// Wall-clock milliseconds since the Unix epoch, or `0` if the clock predates
/// it (the value only feeds heartbeat freshness, which saturates on a backwards
/// clock).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests;
