use super::*;

/// Helper: create a temp dir and return its path.
fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"))
}

// ── Allow-list tests ────────────────────────────────────────────────────

#[test]
fn reject_binary_not_on_allow_list() {
    let sup = AgentSupervisor::new(&[PathBuf::from("/bin/echo")]);
    let result = sup.validate_binary(Path::new("/bin/cat"));
    assert!(matches!(result, Err(Error::NotAllowed { .. })), "expected NotAllowed, got {result:?}");
}

#[test]
fn accept_binary_on_allow_list() {
    let sup = AgentSupervisor::new(&[PathBuf::from("/bin/echo")]);
    let result = sup.validate_binary(Path::new("/bin/echo"));
    assert!(result.is_ok(), "expected Ok, got {result:?}");
}

#[test]
fn symlink_resolved_against_allow_list() {
    let dir = tmp();
    let target = PathBuf::from("/bin/echo");
    let link = dir.path().join("my-echo");

    std::os::unix::fs::symlink(&target, &link).unwrap_or_else(|e| panic!("symlink: {e}"));

    // Allow-list has the real binary; symlink should resolve to it.
    let sup = AgentSupervisor::new(&[target]);
    let result = sup.validate_binary(&link);
    assert!(result.is_ok(), "symlink should resolve to allowed binary");
}

#[test]
fn dotdot_resolved_against_allow_list() {
    let sup = AgentSupervisor::new(&[PathBuf::from("/bin/echo")]);
    let result = sup.validate_binary(Path::new("/bin/../bin/echo"));
    assert!(result.is_ok(), ".. should resolve to allowed path");
}

// ── Spawn + stop tests ──────────────────────────────────────────────────

#[test]
fn spawn_and_stop() {
    let folder = tmp();
    let mut sup = AgentSupervisor::new(&[PathBuf::from("/bin/sleep")]);

    let pid = sup
        .spawn("a1".into(), Path::new("/bin/sleep"), folder.path(), &["60"])
        .unwrap_or_else(|e| panic!("spawn: {e}"));
    assert_eq!(sup.len(), 1);

    let raw = Pid::from_raw(i32::try_from(pid).unwrap_or(i32::MAX));
    assert!(pid_alive(raw), "spawned process should be alive");

    sup.stop("a1").unwrap_or_else(|e| panic!("stop: {e}"));
    assert!(sup.is_empty());

    thread::sleep(Duration::from_millis(50));
    assert!(!pid_alive(raw), "process should be dead after stop");
}

#[test]
fn spawn_rejects_duplicate_id() {
    let folder = tmp();
    let mut sup = AgentSupervisor::new(&[PathBuf::from("/bin/sleep")]);

    let _pid =
        sup.spawn("a1".into(), Path::new("/bin/sleep"), folder.path(), &["60"]).unwrap_or_else(|e| panic!("{e}"));

    let dup = sup.spawn("a1".into(), Path::new("/bin/sleep"), folder.path(), &["60"]);
    assert!(matches!(dup, Err(Error::AlreadySupervised { .. })), "duplicate spawn should fail");

    let _stopped = sup.stop("a1");
}

// ── Adopt + check_liveness tests ────────────────────────────────────────

#[test]
fn adopt_and_detect_vanish() {
    let folder = tmp();
    let mut sup = AgentSupervisor::new(&[]);

    // Spawn a short-lived process externally, then adopt it.
    let mut child = Command::new("/bin/sleep")
        .arg("0")
        .current_dir(folder.path())
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn: {e}"));

    let pid = child.id();
    // Reap it ourselves: an unreaped zombie still answers `kill(pid, 0)` as
    // alive, so the pid must be fully released before the liveness probe.
    let _status = child.wait();
    thread::sleep(Duration::from_millis(200));

    let entry = Entry {
        schema_version: 1,
        id: "a2".into(),
        folder: folder.path().to_string_lossy().into_owned(),
        pid,
        boot_id: "boot-a2".into(),
        model: "test-model".into(),
        protocol_version: 1,
        binary_version: "0.1.0".into(),
        socket_path: String::new(),
        oplog_path: String::new(),
        heartbeat_path: String::new(),
        cap_token: String::new(),
        started_at_ms: 0,
        status: cp_wire::types::registry::AgentStatus::Running,
    };

    sup.adopt("a2".into(), &entry, PathBuf::from("/bin/sleep")).unwrap_or_else(|e| panic!("adopt: {e}"));
    assert_eq!(sup.len(), 1);

    let events = sup.check_liveness();
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], Event::Vanished { agent_id } if agent_id == "a2"),
        "expected Vanished, got {events:?}"
    );
    assert!(sup.is_empty(), "dead agent should be removed");
}

// ── Restart test ────────────────────────────────────────────────────────

#[test]
fn restart_stops_and_respawns() {
    let folder = tmp();
    let mut sup = AgentSupervisor::new(&[PathBuf::from("/bin/sleep")]);

    let pid1 = sup
        .spawn("a3".into(), Path::new("/bin/sleep"), folder.path(), &["60"])
        .unwrap_or_else(|e| panic!("spawn: {e}"));

    let pid2 = sup.restart("a3").unwrap_or_else(|e| panic!("restart: {e}"));

    assert_ne!(pid1, pid2, "restart should yield a new pid");
    assert_eq!(sup.len(), 1);

    let raw1 = Pid::from_raw(i32::try_from(pid1).unwrap_or(i32::MAX));
    thread::sleep(Duration::from_millis(50));
    assert!(!pid_alive(raw1), "old pid should be dead");

    let raw2 = Pid::from_raw(i32::try_from(pid2).unwrap_or(i32::MAX));
    assert!(pid_alive(raw2), "new pid should be alive");

    let _stopped = sup.stop("a3");
}

// ── check_liveness reaps spawned children ───────────────────────────────

#[test]
fn check_liveness_reaps_exited_child() {
    let folder = tmp();
    let mut sup = AgentSupervisor::new(&[PathBuf::from("/bin/sleep")]);

    let _pid =
        sup.spawn("a4".into(), Path::new("/bin/sleep"), folder.path(), &["0"]).unwrap_or_else(|e| panic!("spawn: {e}"));

    thread::sleep(Duration::from_millis(200));

    let events = sup.check_liveness();
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], Event::Exited { agent_id, .. } if agent_id == "a4"),
        "expected Exited, got {events:?}"
    );
    assert!(sup.is_empty());
}

#[test]
fn stop_not_found() {
    let mut sup = AgentSupervisor::new(&[]);
    let result = sup.stop("nonexistent");
    assert!(matches!(result, Err(Error::NotFound { .. })), "expected NotFound");
}
