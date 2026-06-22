//! [`AgentRegistry`] — fleet discovery via directory scan-and-diff
//! (design doc §10, roadmap P5-T1).
//!
//! An agent advertises itself by atomically writing
//! `~/.context-pilot/agents/<id>.json` at boot (`cp-mod-bridge`'s registry
//! writer) and rewriting `<folder>/heartbeat` at a fixed cadence
//! (`cp_wire::heartbeat`). [`AgentRegistry`] reads that directory, derives a
//! [`Liveness`] verdict per record (see [`liveness`]), and diffs each
//! pass against the last to emit fleet-change [`Event`]s.
//!
//! # Scan-and-diff, not a kernel watch
//!
//! Discovery is **poll-based** ([`AgentRegistry::scan`]): each pass reads the
//! directory, parses every record, computes each verdict, and diffs the result
//! against the previous pass. Agents appear and disappear rarely (boot /
//! shutdown), so a directory poll at a modest cadence meets the "within one
//! cadence" latency target without the per-file watch budget that the *oplog*
//! tail (a high-frequency stream, design doc I12) genuinely needs. Keeping the
//! core a pure scan+diff also makes it testable against real files and pids with
//! no timing flakiness — the live driver is a thin loop that calls
//! [`scan`](AgentRegistry::scan) and [`reap_tmp`](AgentRegistry::reap_tmp) each
//! tick.
//!
//! A registry write is `tmp → fsync → rename`, so a crashed writer can leave a
//! `*.tmp` orphan. [`reap_tmp`](AgentRegistry::reap_tmp) deletes those once they
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
pub const DEFAULT_TMP_GRACE: Duration = Duration::from_secs(60);

/// A change in the fleet observed between two [`scan`](AgentRegistry::scan)es.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A record with a previously-unseen id was discovered. Carries the full
    /// record; the agent's liveness is queryable via
    /// [`AgentRegistry::liveness`].
    Appeared(Entry),

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
/// Construct with [`new`](AgentRegistry::new), then call
/// [`scan`](AgentRegistry::scan) on a cadence to drive [`Event`]s and
/// [`reap_tmp`](AgentRegistry::reap_tmp) to clear crash-orphan writes.
#[derive(Debug)]
pub struct AgentRegistry {
    /// Directory holding `<id>.json` records (and transient `*.tmp` writes).
    dir: PathBuf,

    /// Heartbeat freshness window applied by the liveness verdict.
    max_age: Duration,

    /// Last-observed state per agent id, for diffing the next scan.
    known: HashMap<String, Snapshot>,
}

impl AgentRegistry {
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

        // Disappearances: known ids absent from the fresh scan.
        for id in self.known.keys() {
            if !fresh.contains_key(id) {
                events.push(Event::Disappeared(id.clone()));
            }
        }

        for (id, snap) in fresh {
            match self.known.get(id) {
                None => events.push(Event::Appeared(snap.entry.clone())),
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
        for entry in read_dir {
            let entry = entry?;
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
    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
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
mod tests {
    use super::*;
    use cp_wire::heartbeat::HEARTBEAT_SCHEMA_VERSION;
    use tempfile::tempdir;

    /// A boot id of the exact 32-hex-char width the heartbeat record requires.
    const BOOT_A: &str = "0123456789abcdef0123456789abcdef";

    /// A pid that cannot name a live process (above any platform's pid_max).
    const DEAD_PID: u32 = 4_000_000_000;

    fn entry(id: &str, pid: u32, hb_path: &Path, status: AgentStatus) -> Entry {
        Entry {
            schema_version: 1,
            id: id.to_owned(),
            folder: "/tmp/agent".to_owned(),
            pid,
            boot_id: BOOT_A.to_owned(),
            model: "test-model".to_owned(),
            protocol_version: 1,
            binary_version: "0.0.0".to_owned(),
            socket_path: "/tmp/agent/stream.sock".to_owned(),
            oplog_path: "/tmp/agent/oplog".to_owned(),
            heartbeat_path: hb_path.to_string_lossy().into_owned(),
            cap_token: "tok".to_owned(),
            started_at_ms: 0,
            status,
        }
    }

    fn heartbeat(pid: u32, timestamp_ms: u64) -> Heartbeat {
        Heartbeat {
            schema_version: HEARTBEAT_SCHEMA_VERSION,
            timestamp_ms,
            sequence: 0,
            pid,
            boot_id: BOOT_A.to_owned(),
        }
    }

    /// Write `record` as `<id>.json` into `dir`.
    fn write_record(dir: &Path, record: &Entry) {
        let path = dir.join(format!("{}{RECORD_SUFFIX}", record.id));
        fs::write(path, serde_json::to_vec(record).expect("serialize")).expect("write record");
    }

    /// Write `hb` to `path` so a verdict can read a real, decodable beat.
    fn write_heartbeat(path: &Path, hb: &Heartbeat) {
        fs::write(path, hb.encode().expect("encode")).expect("write heartbeat");
    }

    #[test]
    fn scan_emits_appeared_then_disappeared() {
        let dir = tempdir().expect("dir");
        let me = std::process::id();
        let hb_path = dir.path().join("hb-a");
        write_heartbeat(&hb_path, &heartbeat(me, now_ms()));
        write_record(dir.path(), &entry("a", me, &hb_path, AgentStatus::Running));

        let mut reg = AgentRegistry::new(dir.path().to_path_buf());
        let first = reg.scan().expect("scan");
        assert_eq!(first.len(), 1);
        assert!(matches!(first.first(), Some(Event::Appeared(e)) if e.id == "a"));
        assert_eq!(reg.liveness("a"), Some(Liveness::Live), "fresh self-pid agent is live");

        // A second scan with no changes is quiet.
        assert!(reg.scan().expect("scan").is_empty(), "idempotent scan emits nothing");

        // Remove the record → Disappeared.
        fs::remove_file(dir.path().join("a.json")).expect("rm");
        let third = reg.scan().expect("scan");
        assert_eq!(third, vec![Event::Disappeared("a".to_owned())]);
        assert!(reg.is_empty());
    }

    #[test]
    fn scan_emits_status_change() {
        let dir = tempdir().expect("dir");
        let me = std::process::id();
        let hb_path = dir.path().join("hb-a");
        write_heartbeat(&hb_path, &heartbeat(me, now_ms()));
        write_record(dir.path(), &entry("a", me, &hb_path, AgentStatus::Starting));

        let mut reg = AgentRegistry::new(dir.path().to_path_buf());
        let _appeared = reg.scan().expect("scan");

        write_record(dir.path(), &entry("a", me, &hb_path, AgentStatus::Running));
        let events = reg.scan().expect("scan");
        assert_eq!(events, vec![Event::StatusChanged("a".to_owned(), AgentStatus::Running)]);
    }

    #[test]
    fn scan_emits_stale_on_liveness_loss() {
        let dir = tempdir().expect("dir");
        let me = std::process::id();
        let hb_path = dir.path().join("hb-a");
        // Start live (fresh beat).
        write_heartbeat(&hb_path, &heartbeat(me, now_ms()));
        write_record(dir.path(), &entry("a", me, &hb_path, AgentStatus::Running));

        let mut reg = AgentRegistry::new(dir.path().to_path_buf());
        let _appeared = reg.scan().expect("scan");
        assert_eq!(reg.liveness("a"), Some(Liveness::Live));

        // Rewrite the heartbeat far in the past → it goes stale.
        write_heartbeat(&hb_path, &heartbeat(me, 0));
        let events = reg.scan().expect("scan");
        assert_eq!(events, vec![Event::Stale("a".to_owned(), Liveness::StaleHeartbeat)]);
        assert_eq!(reg.liveness("a"), Some(Liveness::StaleHeartbeat));
    }

    #[test]
    fn scan_reports_dead_pid_entry_as_stale_not_live() {
        // The pid-reused / crashed-without-cleanup case: a record whose pid is
        // not a live process must be reported stale, never live.
        let dir = tempdir().expect("dir");
        let hb_path = dir.path().join("hb-a");
        write_heartbeat(&hb_path, &heartbeat(DEAD_PID, now_ms()));
        write_record(dir.path(), &entry("a", DEAD_PID, &hb_path, AgentStatus::Running));

        let mut reg = AgentRegistry::new(dir.path().to_path_buf());
        let events = reg.scan().expect("scan");
        assert!(matches!(events.first(), Some(Event::Appeared(_))));
        assert_eq!(reg.liveness("a"), Some(Liveness::StalePid), "dead pid → stale, not live");
    }

    #[test]
    fn scan_skips_unparseable_and_tmp_files() {
        let dir = tempdir().expect("dir");
        fs::write(dir.path().join("garbage.json"), b"not json").expect("write");
        fs::write(dir.path().join("a.json.tmp"), b"{}").expect("write tmp");

        let mut reg = AgentRegistry::new(dir.path().to_path_buf());
        assert!(reg.scan().expect("scan").is_empty(), "garbage + tmp yield no agents");
    }

    #[test]
    fn reap_tmp_collects_aged_orphans_only() {
        let dir = tempdir().expect("dir");
        let tmp = dir.path().join("x.json.tmp");
        fs::write(&tmp, b"partial").expect("write tmp");

        let reg = AgentRegistry::new(dir.path().to_path_buf());
        // A long grace protects the just-written tmp (an in-flight write).
        assert_eq!(reg.reap_tmp(DEFAULT_TMP_GRACE).expect("reap"), 0);
        assert!(tmp.exists(), "young tmp survives");

        // Zero grace makes any aged file eligible → the orphan is collected.
        assert_eq!(reg.reap_tmp(Duration::ZERO).expect("reap"), 1);
        assert!(!tmp.exists(), "aged orphan reaped");
    }

    #[test]
    fn empty_or_missing_dir_scans_clean() {
        let dir = tempdir().expect("dir");
        let mut reg = AgentRegistry::new(dir.path().join("does-not-exist"));
        assert!(reg.scan().expect("scan").is_empty());
        assert_eq!(reg.reap_tmp(DEFAULT_TMP_GRACE).expect("reap"), 0);
    }
}
