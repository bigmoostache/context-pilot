//! Unit tests for [`FleetScanner`](super::FleetScanner), split from `mod.rs`
//! to keep that file within the 500-line cap (the supervisor/tests.rs precedent).

use super::*;
use tempfile::tempdir;

/// A boot id of the exact 32-hex-char width the heartbeat record requires.
const BOOT_A: &str = "0123456789abcdef0123456789abcdef";

/// A second, distinct boot id — models an agent rebooting under the same id.
const BOOT_B: &str = "ffffffffffffffffffffffffffffffff";

/// A pid that cannot name a live process (above any platform's `pid_max`).
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
        tee_socket_path: "/tmp/agent/tee.sock".to_owned(),
        cap_token: "tok".to_owned(),
        started_at_ms: 0,
        status,
    }
}

fn heartbeat(pid: u32, timestamp_ms: u64) -> Heartbeat {
    Heartbeat::new(timestamp_ms, 0, pid, BOOT_A.to_owned())
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

/// Assert `events` is a single `Appeared` for `id` and that `id` reads back
/// live. A plain call, so hoisting the `matches!` guard out of the test body
/// keeps `scan_emits_appeared_then_disappeared` under the cognitive cap.
fn assert_appeared_live(reg: &FleetScanner, events: &[Event], id: &str) {
    assert_eq!(events.len(), 1);
    assert!(matches!(events.first(), Some(Event::Appeared(e)) if e.id == id));
    assert_eq!(reg.liveness(id), Some(Liveness::Live), "fresh self-pid agent is live");
}

#[test]
fn scan_emits_appeared_then_disappeared() {
    let dir = tempdir().expect("dir");
    let me = std::process::id();
    let hb_path = dir.path().join("hb-a");
    write_heartbeat(&hb_path, &heartbeat(me, now_ms()));
    write_record(dir.path(), &entry("a", me, &hb_path, AgentStatus::Running));

    let mut reg = FleetScanner::new(dir.path().to_path_buf());
    let first = reg.scan().expect("scan");
    assert_appeared_live(&reg, &first, "a");

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

    let mut reg = FleetScanner::new(dir.path().to_path_buf());
    let _appeared = reg.scan().expect("scan");

    write_record(dir.path(), &entry("a", me, &hb_path, AgentStatus::Running));
    let events = reg.scan().expect("scan");
    assert_eq!(events, vec![Event::StatusChanged("a".to_owned(), AgentStatus::Running)]);
}

#[test]
fn scan_rebuilds_readers_on_boot_id_change() {
    // An agent that reboots keeps its id but mints a fresh boot_id. The scanner
    // must treat that as a tear-down + rebuild (Disappeared then Appeared) so
    // the driver re-points its per-agent readers at the possibly-new paths
    // (T713 sync-dir migration / any reload) — otherwise the tee stays
    // disconnected and the view freezes on the previous generation.
    let dir = tempdir().expect("dir");
    let me = std::process::id();
    let hb_path = dir.path().join("hb-a");
    write_heartbeat(&hb_path, &heartbeat(me, now_ms()));
    write_record(dir.path(), &entry("a", me, &hb_path, AgentStatus::Running));

    let mut reg = FleetScanner::new(dir.path().to_path_buf());
    let _appeared = reg.scan().expect("scan"); // first sight → Appeared

    // Same id, fresh boot_id → the reboot signal.
    let mut rebooted = entry("a", me, &hb_path, AgentStatus::Running);
    rebooted.boot_id = BOOT_B.to_owned();
    write_record(dir.path(), &rebooted);

    let events = reg.scan().expect("scan");
    assert_eq!(
        events,
        vec![Event::Disappeared("a".to_owned()), Event::Appeared(Box::new(rebooted))],
        "a boot_id change must tear down + rebuild via Disappeared then Appeared",
    );
}

#[test]
fn scan_emits_stale_on_liveness_loss() {
    let dir = tempdir().expect("dir");
    let me = std::process::id();
    let hb_path = dir.path().join("hb-a");
    // Start live (fresh beat).
    write_heartbeat(&hb_path, &heartbeat(me, now_ms()));
    write_record(dir.path(), &entry("a", me, &hb_path, AgentStatus::Running));

    let mut reg = FleetScanner::new(dir.path().to_path_buf());
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

    let mut reg = FleetScanner::new(dir.path().to_path_buf());
    let events = reg.scan().expect("scan");
    assert!(matches!(events.first(), Some(Event::Appeared(_))));
    assert_eq!(reg.liveness("a"), Some(Liveness::StalePid), "dead pid \u{2192} stale, not live");
}

#[test]
fn scan_skips_unparseable_and_tmp_files() {
    let dir = tempdir().expect("dir");
    fs::write(dir.path().join("garbage.json"), b"not json").expect("write");
    fs::write(dir.path().join("a.json.tmp"), b"{}").expect("write tmp");

    let mut reg = FleetScanner::new(dir.path().to_path_buf());
    assert!(reg.scan().expect("scan").is_empty(), "garbage + tmp yield no agents");
}

#[test]
fn reap_tmp_collects_aged_orphans_only() {
    let dir = tempdir().expect("dir");
    let tmp = dir.path().join("x.json.tmp");
    fs::write(&tmp, b"partial").expect("write tmp");

    let reg = FleetScanner::new(dir.path().to_path_buf());
    // A long grace protects the just-written tmp (an in-flight write).
    assert_eq!(reg.reap_tmp(DEFAULT_TMP_GRACE).expect("reap"), 0);
    assert!(tmp.exists(), "young tmp survives");

    // Zero grace makes any aged file eligible → the orphan is collected.
    assert_eq!(reg.reap_tmp(Duration::ZERO).expect("reap"), 1);
    assert!(!tmp.exists(), "aged orphan reaped");
}

#[test]
fn reap_orphan_sync_dirs_collects_only_unregistered_aged_dirs() {
    // Register one live agent "a"; its sync dir must be preserved.
    let agents = tempdir().expect("agents");
    let me = std::process::id();
    let hb_path = agents.path().join("hb-a");
    write_heartbeat(&hb_path, &heartbeat(me, now_ms()));
    write_record(agents.path(), &entry("a", me, &hb_path, AgentStatus::Running));
    let mut reg = FleetScanner::new(agents.path().to_path_buf());
    let _appeared = reg.scan().expect("scan");

    // A sibling sync root with the registered agent's dir + an orphan.
    let sync_root = agents.path().join("..").join("sync-test-root");
    fs::create_dir_all(sync_root.join("a")).expect("mkdir a");
    fs::create_dir_all(sync_root.join("ghost")).expect("mkdir ghost");

    // A long grace protects everything (both dirs just created).
    assert_eq!(reg.reap_orphan_sync_dirs(&sync_root, DEFAULT_SYNC_ORPHAN_GRACE).expect("reap"), 0);
    assert!(sync_root.join("a").exists() && sync_root.join("ghost").exists(), "young dirs survive");

    // Zero grace: only the UNREGISTERED dir is collected; "a" is kept.
    assert_eq!(reg.reap_orphan_sync_dirs(&sync_root, Duration::ZERO).expect("reap"), 1);
    assert!(sync_root.join("a").exists(), "a registered agent's sync dir is preserved");
    assert!(!sync_root.join("ghost").exists(), "an orphan sync dir past grace is reaped");

    let _cleanup = fs::remove_dir_all(&sync_root);
}

#[test]
fn empty_or_missing_dir_scans_clean() {
    let dir = tempdir().expect("dir");
    let mut reg = FleetScanner::new(dir.path().join("does-not-exist"));
    assert!(reg.scan().expect("scan").is_empty());
    assert_eq!(reg.reap_tmp(DEFAULT_TMP_GRACE).expect("reap"), 0);
}
