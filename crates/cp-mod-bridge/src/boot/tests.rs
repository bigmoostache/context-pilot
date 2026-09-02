//! Unit tests for [`Boot`](super::Boot) — split into this sibling file to keep
//! `boot/mod.rs` within the 500-line budget. A child `#[cfg(test)]` module can
//! reach its parent's private items (`start_inner`, `BootDirs`, the file-name
//! constants) through `use super::*`.

use super::*;
use tempfile::tempdir;

/// Boot into temp folders so tests never touch the real home or cwd.
fn boot(folder: &Path, agents: &Path) -> BootResult<Boot> {
    Boot::start_in(folder, agents, "test-model")
}

/// The per-agent sync dir `start_in` derives for `agents` + `id` (the `sync`
/// sibling of the agents dir — mirrors production's `~/.context-pilot/{agents,sync}`).
fn sync_dir_for(agents: &Path, id: &str) -> PathBuf {
    agents.parent().map_or_else(|| agents.join("sync"), |parent| parent.join("sync")).join(id)
}

#[test]
fn boot_acquires_all_resources() {
    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents");
    let booted = boot(folder.path(), agents.path()).expect("boot");
    assert_boot_disk(&booted, folder.path(), agents.path());
    assert_boot_entry(&booted);
}

/// Assert the on-disk artifacts land under the SYNC dir (T713), not the realm
/// folder — split from the advertised-path checks to stay under the
/// cognitive-complexity cap.
fn assert_boot_disk(booted: &Boot, folder: &Path, agents: &Path) {
    assert!(registry::path(agents, booted.id()).exists(), "registry record written");
    let sync_dir = sync_dir_for(agents, booted.id());
    assert!(sync_dir.join(SOCKET_FILE).exists(), "socket bound under sync dir");
    assert!(sync_dir.join(OPLOG_DIR).exists(), "oplog dir created under sync dir");
    assert!(!folder.join(SOCKET_FILE).exists(), "realm folder stays clean");
}

/// Assert the advertised registry paths point inside the sync dir + the minted
/// secrets are the expected width.
fn assert_boot_entry(booted: &Boot) {
    assert!(booted.entry().oplog_path.ends_with(OPLOG_DIR));
    assert!(booted.entry().tee_socket_path.ends_with(TEE_SOCKET));
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
    // Simulate a crash leaving a stale socket file at the bind path — which is
    // now under the per-agent sync dir (T713), not the realm folder.
    fs::create_dir_all(folder.path()).expect("mkdir");
    let canonical = fs::canonicalize(folder.path()).expect("canon");
    let id = folder_id(&canonical.to_string_lossy());
    let sync_dir = sync_dir_for(agents.path(), &id);
    fs::create_dir_all(&sync_dir).expect("mkdir sync");
    fs::write(sync_dir.join(SOCKET_FILE), b"stale").expect("stale socket");

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

    // A fail-fast attempt against the same folder must refuse *immediately* (no
    // ~2s retry wait) with `AlreadyRunning` — the background recovery path that
    // must never stall the main loop.
    let started = Instant::now();
    let sync_root = agents.path().parent().map_or_else(|| agents.path().join("sync"), |p| p.join("sync"));
    let dirs = BootDirs { agents_dir: agents.path(), sync_root: &sync_root };
    let contended = Boot::start_inner(folder.path(), dirs, "test-model", 0);
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
    let recovered = Boot::start_inner(folder.path(), dirs, "test-model", 0);
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
    let canonical = fs::canonicalize(folder.path()).expect("canon");
    let oplog_dir = sync_dir_for(agents.path(), &folder_id(&canonical.to_string_lossy())).join(OPLOG_DIR);

    // Boot emits Lifecycle::Running; dropping it emits Lifecycle::Stopping. The
    // drop joins the oplog commit thread, draining + fsyncing both records
    // before it returns, so reading after the drop is race-free.
    let booted = boot(folder.path(), agents.path()).expect("boot");
    drop(booted);

    let entries = read_all_entries(&oplog_dir);
    let has_running =
        entries.iter().any(|e| matches!(e.kind, OpEntryKind::Lifecycle { state: LifecycleState::Running }));
    let has_stopping =
        entries.iter().any(|e| matches!(e.kind, OpEntryKind::Lifecycle { state: LifecycleState::Stopping }));

    assert!(has_running, "Lifecycle::Running must be journaled at boot");
    assert!(has_stopping, "Lifecycle::Stopping must be journaled on graceful drop");
}
