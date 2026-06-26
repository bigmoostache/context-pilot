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
