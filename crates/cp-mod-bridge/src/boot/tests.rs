//! Boot sequence tests, split out of [`super`] to keep that module within the
//! workspace's 500-line budget (the same prod/tests split the orchestrator's
//! `supervisor` module uses).
//!
//! Every test boots into temporary folders via the local [`boot`] helper, so a
//! run never touches the real `$HOME` or the working directory's `bridge.lock`.

use super::*;
use tempfile::tempdir;

/// Boot into temp folders so tests never touch the real home or cwd.
fn boot(folder: &Path, agents: &Path) -> BootResult<Boot> {
    Boot::start_in(folder, agents, "test-model")
}

#[test]
fn boot_acquires_all_resources() {
    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents");
    let booted = boot(folder.path(), agents.path()).expect("boot");

    // Registry record exists, 0600, and round-trips.
    let registry = registry::path(agents.path(), booted.id());
    assert!(registry.exists(), "registry record written");

    // Socket bound, oplog dir created.
    assert!(folder.path().join(SOCKET_FILE).exists(), "socket bound");
    assert!(folder.path().join(OPLOG_DIR).exists(), "oplog dir created");

    // The advertised paths are inside the canonical folder.
    assert!(booted.entry().oplog_path.ends_with(OPLOG_DIR));
    assert_eq!(booted.entry().protocol_version, PROTOCOL_VERSION);
    assert_eq!(booted.cap_token().len(), 64, "256-bit token");
}

#[test]
fn second_boot_same_folder_refuses() {
    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents");
    let _first = boot(folder.path(), agents.path()).expect("first boot");

    let second = boot(folder.path(), agents.path());
    assert!(
        matches!(second, Err(Error::AlreadyRunning { .. })),
        "a second instance in the same folder must be refused, got {second:?}",
    );
}

#[test]
fn boot_releases_lock_on_drop() {
    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents");
    {
        let _first = boot(folder.path(), agents.path()).expect("first boot");
    } // dropped here → lock released, registry + socket removed.

    // A fresh boot in the same folder now succeeds.
    let again = boot(folder.path(), agents.path());
    assert!(again.is_ok(), "lock must be released on drop, got {again:?}");
}

#[test]
fn drop_keeps_registry_record() {
    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents");
    let registry;
    {
        let booted = boot(folder.path(), agents.path()).expect("boot");
        registry = registry::path(agents.path(), booted.id());
        assert!(registry.exists());
    }
    assert!(registry.exists(), "registry record must survive graceful drop (agent shows as Disconnected)");
}

#[test]
fn stale_socket_is_replaced() {
    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents");
    // Simulate a crash leaving a stale socket file.
    fs::create_dir_all(folder.path()).expect("mkdir");
    fs::write(folder.path().join(SOCKET_FILE), b"stale").expect("stale socket");

    let booted = boot(folder.path(), agents.path());
    assert!(booted.is_ok(), "a stale socket must be unlinked and rebound, got {booted:?}");
}

#[test]
fn try_start_fails_fast_when_locked_then_recovers_when_freed() {
    use std::time::Instant;

    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents");

    // A patient boot holds the lock.
    let first = boot(folder.path(), agents.path()).expect("first boot");

    // A fail-fast `try_start` against the same folder must refuse
    // *immediately* (no ~2s retry wait) with `AlreadyRunning` — this is the
    // background recovery path that must never stall the main loop.
    let started = Instant::now();
    let contended = Boot::start_inner(folder.path(), agents.path(), "test-model", 0);
    let elapsed = started.elapsed();
    assert!(
        matches!(contended, Err(Error::AlreadyRunning { .. })),
        "a contended fail-fast attempt must refuse, got {contended:?}",
    );
    assert!(
        elapsed < lock::LOCK_RETRY_BACKOFF,
        "fail-fast must return well under one backoff ({elapsed:?}), not sleep out the retry window",
    );

    // Once the holder releases the lock, the next fail-fast attempt wins —
    // modelling the bridge recovering mid-session after a dying predecessor
    // finally frees the lock.
    drop(first);
    let recovered = Boot::start_inner(folder.path(), agents.path(), "test-model", 0);
    assert!(recovered.is_ok(), "fail-fast must succeed once the lock is free, got {recovered:?}");
}

/// Read every cleanly-decoded oplog entry across all segments in `dir`.
fn read_all_entries(dir: &Path) -> Vec<cp_wire::types::oplog::OpEntry> {
    let mut out = Vec::new();
    for idx in cp_oplog::segment::indices(dir).unwrap_or_default() {
        if let Ok(scan) = cp_oplog::segment::read(&cp_oplog::segment::path(dir, idx)) {
            out.extend(scan.entries);
        }
    }
    out
}

#[test]
fn lifecycle_running_on_boot_and_stopping_on_drop() {
    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents");
    let oplog_dir = fs::canonicalize(folder.path()).expect("canon").join(OPLOG_DIR);

    // Boot emits Lifecycle::Running; dropping it emits Lifecycle::Stopping.
    // The drop joins the oplog commit thread, draining + fsyncing both
    // records before it returns, so reading after the drop is race-free.
    let booted = boot(folder.path(), agents.path()).expect("boot");
    drop(booted);

    let lifecycles: Vec<LifecycleState> = read_all_entries(&oplog_dir)
        .iter()
        .filter_map(|e| match &e.kind {
            OpEntryKind::Lifecycle { state } => Some(*state),
            _ => None,
        })
        .collect();

    assert!(
        lifecycles.contains(&LifecycleState::Running),
        "Lifecycle::Running must be journaled at boot, got {lifecycles:?}",
    );
    assert!(
        lifecycles.contains(&LifecycleState::Stopping),
        "Lifecycle::Stopping must be journaled on graceful drop, got {lifecycles:?}",
    );
}
