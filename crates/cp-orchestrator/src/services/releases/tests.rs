use super::*;

#[test]
fn semver_sort_key_ordering() {
    let mut tags = ["v0.2.9", "v0.2.10", "v0.1.0", "v0.2.2", "v0.2.1"];
    tags.sort_by(|a, b| semver_sort_key(b).cmp(&semver_sort_key(a)));
    assert_eq!(tags, ["v0.2.10", "v0.2.9", "v0.2.2", "v0.2.1", "v0.1.0"]);
}

#[test]
fn detect_arch_returns_known_value() {
    let arch = detect_arch();
    // Must be one of the known targets (or unknown-unknown on exotic).
    assert!(arch.contains('-'), "arch should be os-arch: {arch}");
}

#[test]
fn store_load_default_config() {
    let dir = std::env::temp_dir().join(format!("cp-rel-test-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let store = ReleaseStore::load(dir.clone());
    assert!(store.is_arch_auto());
    assert!(store.active_tag().is_none());
    assert!(store.local_releases().is_empty());

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn store_set_arch_persists() {
    let dir = std::env::temp_dir().join(format!("cp-rel-arch-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let mut store = ReleaseStore::load(dir.clone());
    store.set_arch("linux-x86_64");
    assert_eq!(store.arch(), "linux-x86_64");
    assert!(!store.is_arch_auto());

    // Reload from disk proves persistence.
    let reloaded = ReleaseStore::load(dir.clone());
    assert_eq!(reloaded.arch(), "linux-x86_64");
    assert!(!reloaded.is_arch_auto());

    // Auto-detect reset.
    let mut store2 = reloaded;
    store2.auto_detect_arch();
    assert!(store2.is_arch_auto());

    drop(std::fs::remove_dir_all(&dir));
}

/// V4.1a — a legacy `config.json` (arch + active tag only, no update-policy
/// fields) loads with the v3 defaults and no error.
#[test]
fn update_config_legacy_migrates_with_defaults() {
    let dir = std::env::temp_dir().join(format!("cp-rel-legacy-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));
    std::fs::write(dir.join("config.json"), br#"{"arch":"linux-aarch64","arch_auto":false,"active_tag":"v0.3.0"}"#)
        .expect("write legacy config");

    let store = ReleaseStore::load(dir.clone());
    assert_eq!(store.arch(), "linux-aarch64", "legacy fields preserved");
    assert_eq!(store.active_tag(), Some("v0.3.0"));
    assert_eq!(store.update_mode(), UpdateMode::Auto, "default mode is auto");
    assert_eq!(store.channel(), "stable");
    assert_eq!(store.poll_interval_hours(), 6);
    assert_eq!(store.window(), &MaintenanceWindow::default(), "default window 03:00–05:00");

    drop(std::fs::remove_dir_all(&dir));
}

/// V4.1b — the new fields round-trip through persist + reload.
#[test]
fn update_config_roundtrip() {
    let dir = std::env::temp_dir().join(format!("cp-rel-upcfg-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let mut store = ReleaseStore::load(dir.clone());
    store.set_update_mode(UpdateMode::Manual);
    let window = MaintenanceWindow { start: "22:30".to_owned(), end: "23:45".to_owned() };
    store.set_window(window.clone()).expect("valid window accepted");
    assert!(
        store.set_window(MaintenanceWindow { start: "9:99".to_owned(), end: "05:00".to_owned() }).is_err(),
        "malformed window rejected"
    );

    let reloaded = ReleaseStore::load(dir.clone());
    assert_eq!(reloaded.update_mode(), UpdateMode::Manual);
    assert_eq!(reloaded.window(), &window, "rejected write must not have clobbered the valid one");

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn store_select_rejects_missing() {
    let dir = std::env::temp_dir().join(format!("cp-rel-sel-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let mut store = ReleaseStore::load(dir.clone());
    assert!(store.select("v0.0.1-ghost").is_err());

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn store_delete_rejects_active() {
    let dir = std::env::temp_dir().join(format!("cp-rel-del-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    // Create a fake release directory with a binary.
    let tag_dir = dir.join("v0.1.0-test");
    drop(std::fs::create_dir_all(&tag_dir));
    drop(std::fs::write(tag_dir.join("cpilot"), b"fake"));

    let mut store = ReleaseStore::load(dir.clone());
    let _binary = store.select("v0.1.0-test").expect("select should succeed");
    assert!(store.delete("v0.1.0-test").is_err(), "cannot delete active");

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn local_releases_scan() {
    let dir = std::env::temp_dir().join(format!("cp-rel-scan-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    // Create two fake releases.
    for tag in ["v0.1.0-aaa", "v0.2.0-bbb"] {
        let tag_dir = dir.join(tag);
        drop(std::fs::create_dir_all(&tag_dir));
        drop(std::fs::write(tag_dir.join("cpilot"), b"fake-binary"));
    }
    // A non-tag directory should be ignored.
    drop(std::fs::create_dir_all(dir.join("not-a-release")));

    let store = ReleaseStore::load(dir.clone());
    let locals = store.local_releases();
    assert_eq!(locals.len(), 2);
    // Sorted descending by tag.
    assert_eq!(locals[0].tag, "v0.2.0-bbb");
    assert_eq!(locals[1].tag, "v0.1.0-aaa");
    assert!(locals[0].binary_size > 0);

    drop(std::fs::remove_dir_all(&dir));
}

// ── Self-update (stage / boot_check / boot_commit) ───────────────────────

#[test]
fn stage_swaps_binary_and_writes_markers() {
    let dir = std::env::temp_dir().join(format!("cp-selfupd-stage-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let install = dir.join("cp-orchestrator");
    drop(std::fs::write(&install, b"OLD-BINARY"));
    let src = dir.join("new-orch");
    drop(std::fs::write(&src, b"NEW-BINARY-BYTES"));

    stage_orchestrator_update(&install, &src).expect("stage should succeed");

    // install now holds the new bytes; backup holds the old ones; marker exists.
    assert_eq!(std::fs::read(&install).unwrap(), b"NEW-BINARY-BYTES");
    assert_eq!(std::fs::read(backup_path(&install)).unwrap(), b"OLD-BINARY");
    assert_eq!(std::fs::read_to_string(pending_path(&install)).unwrap(), "0");

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn stage_rejects_empty_source() {
    let dir = std::env::temp_dir().join(format!("cp-selfupd-empty-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let install = dir.join("cp-orchestrator");
    drop(std::fs::write(&install, b"OLD"));
    let src = dir.join("empty-orch");
    drop(std::fs::write(&src, b""));

    assert!(stage_orchestrator_update(&install, &src).is_err(), "empty src rejected");
    // install untouched.
    assert_eq!(std::fs::read(&install).unwrap(), b"OLD");

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn boot_check_rolls_back_after_max_attempts() {
    let dir = std::env::temp_dir().join(format!("cp-selfupd-rollback-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let install = dir.join("cp-orchestrator");
    drop(std::fs::write(&install, b"NEW-CRASHING"));
    drop(std::fs::write(backup_path(&install), b"OLD-GOOD"));
    drop(std::fs::write(pending_path(&install), b"0"));

    // First boot: counter 0 -> 1, still within tolerance (MAX_BOOT_ATTEMPTS=2).
    boot_check(&install);
    assert_eq!(std::fs::read_to_string(pending_path(&install)).unwrap(), "1");
    assert_eq!(std::fs::read(&install).unwrap(), b"NEW-CRASHING", "not yet rolled back");

    // Second boot: counter 1 -> 2 == MAX → rollback to backup, markers cleared.
    boot_check(&install);
    assert_eq!(std::fs::read(&install).unwrap(), b"OLD-GOOD", "rolled back to backup");
    assert!(!pending_path(&install).exists(), "pending marker cleared");

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn boot_commit_clears_markers() {
    let dir = std::env::temp_dir().join(format!("cp-selfupd-commit-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let install = dir.join("cp-orchestrator");
    drop(std::fs::write(&install, b"NEW-GOOD"));
    drop(std::fs::write(backup_path(&install), b"OLD"));
    drop(std::fs::write(pending_path(&install), b"1"));

    boot_commit(&install);
    assert!(!pending_path(&install).exists(), "pending removed");
    assert!(!backup_path(&install).exists(), "backup removed");
    // Live binary is untouched.
    assert_eq!(std::fs::read(&install).unwrap(), b"NEW-GOOD");

    drop(std::fs::remove_dir_all(&dir));
}

/// V2.2a — the probe never turns healthy within the deadline → **no** commit:
/// `.pending` and `.bak` are preserved so the next `boot_check` counts the
/// failed attempt and can roll back.
#[test]
fn self_update_deadline_without_health_leaves_markers() {
    let dir = std::env::temp_dir().join(format!("cp-selfupd-gate-ko-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let install = dir.join("cp-orchestrator");
    drop(std::fs::write(&install, b"NEW"));
    drop(std::fs::write(backup_path(&install), b"OLD"));
    drop(std::fs::write(pending_path(&install), b"1"));

    let committed = boot_commit_when_healthy(
        &install,
        || false, // /healthz answers 503 forever
        std::time::Duration::from_millis(30),
        std::time::Duration::from_millis(5),
    );
    assert!(!committed, "an unhealthy boot must not commit");
    assert!(pending_path(&install).exists(), ".pending preserved for the boot-attempt guard");
    assert!(backup_path(&install).exists(), ".bak preserved for rollback");

    drop(std::fs::remove_dir_all(&dir));
}

/// V2.2b — the probe reports healthy before the deadline → the update is
/// committed: `.pending` and `.bak` both removed.
#[test]
fn self_update_commits_once_healthy() {
    let dir = std::env::temp_dir().join(format!("cp-selfupd-gate-ok-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let install = dir.join("cp-orchestrator");
    drop(std::fs::write(&install, b"NEW-GOOD"));
    drop(std::fs::write(backup_path(&install), b"OLD"));
    drop(std::fs::write(pending_path(&install), b"1"));

    // Healthy on the third poll — the gate must keep polling until then.
    let mut polls = 0u32;
    let committed = boot_commit_when_healthy(
        &install,
        || {
            polls += 1;
            polls >= 3
        },
        std::time::Duration::from_secs(5),
        std::time::Duration::from_millis(2),
    );
    assert!(committed, "a healthy boot commits");
    assert!(!pending_path(&install).exists(), ".pending removed on commit");
    assert!(!backup_path(&install).exists(), ".bak removed on commit");
    assert_eq!(std::fs::read(&install).unwrap(), b"NEW-GOOD", "live binary untouched");

    drop(std::fs::remove_dir_all(&dir));
}

/// A normal boot (no staged update) never polls the probe at all.
#[test]
fn self_update_gate_noop_without_pending() {
    let dir = std::env::temp_dir().join(format!("cp-selfupd-gate-noop-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let install = dir.join("cp-orchestrator");
    drop(std::fs::write(&install, b"NORMAL"));

    let mut polls = 0u32;
    let committed = boot_commit_when_healthy(
        &install,
        || {
            polls += 1;
            true
        },
        std::time::Duration::from_secs(1),
        std::time::Duration::from_millis(1),
    );
    assert!(!committed, "nothing staged, nothing committed");
    assert_eq!(polls, 0, "no probe traffic on a normal boot");

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn boot_check_noop_without_pending() {
    let dir = std::env::temp_dir().join(format!("cp-selfupd-noop-{}", std::process::id()));
    drop(std::fs::create_dir_all(&dir));

    let install = dir.join("cp-orchestrator");
    drop(std::fs::write(&install, b"NORMAL"));

    // No pending marker → boot_check and boot_commit are both no-ops.
    boot_check(&install);
    boot_commit(&install);
    assert_eq!(std::fs::read(&install).unwrap(), b"NORMAL");

    drop(std::fs::remove_dir_all(&dir));
}
