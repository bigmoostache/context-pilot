use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use crate::filehash::{description_key, digest};
use crate::storage::{DescPatch, YamlEntry, apply_patches, populate_with, prune_plan, refresh_with};
use crate::types::TreeFileDescription;

/// Fresh scratch dir unique to `name` (no tempfile dep).
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cp-mod-tree-test-{}-{name}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Write `content` to `dir/name`; returns the path as a string.
fn write(dir: &std::path::Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("write file");
    path.to_string_lossy().into_owned()
}

fn entry(path: &str, description: &str, ms: u64) -> YamlEntry {
    YamlEntry { path: path.to_owned(), description: description.to_owned(), last_edited_ms: ms }
}

fn desc(path: &str, description: &str, file_hash: &str) -> TreeFileDescription {
    TreeFileDescription { path: path.to_owned(), description: description.to_owned(), file_hash: file_hash.to_owned() }
}

#[test]
fn prune_keeps_protected_and_newest_per_path() {
    let mut map = BTreeMap::new();
    for (i, ms) in [10u64, 50, 20, 40, 30].into_iter().enumerate() {
        let _prev = map.insert(format!("a{i}"), entry("a.rs", "old", ms));
    }
    let _prev = map.insert("b0".to_owned(), entry("b.rs", "only", 0));
    // a0 is the oldest but protected (current content).
    let protected: HashSet<String> = HashSet::from(["a0".to_owned()]);
    let drop = prune_plan(&map, &protected, 2);
    // Kept: a0 (protected), a1 (50), a3 (40). Dropped: a2 (20), a4 (30).
    assert_eq!(drop, HashSet::from(["a2".to_owned(), "a4".to_owned()]));
}

#[test]
fn prune_ties_on_legacy_zero_timestamps_are_deterministic() {
    let mut map = BTreeMap::new();
    for key in ["k3", "k1", "k2"] {
        let _prev = map.insert(key.to_owned(), entry("x.rs", "d", 0));
    }
    let drop = prune_plan(&map, &HashSet::new(), 1);
    assert_eq!(drop, HashSet::from(["k2".to_owned(), "k3".to_owned()]));
}

#[test]
fn digest_matches_key_and_refreshes_on_change() {
    let dir = scratch("digest");
    let path = write(&dir, "f.txt", "one");
    let first = digest(std::path::Path::new(&path)).expect("digest");
    assert_eq!(first.desc_key, description_key(&path, b"one"));
    assert_eq!(first.file_hash.len(), 8);
    // Different length guarantees a new stamp even on coarse-mtime filesystems.
    let _same = write(&dir, "f.txt", "three");
    let second = digest(std::path::Path::new(&path)).expect("digest");
    assert_eq!(second.desc_key, description_key(&path, b"three"));
    assert_ne!(first.file_hash, second.file_hash);
    assert!(digest(&dir).is_none(), "directories have no digest");
}

#[test]
fn refresh_restores_description_for_current_content() {
    let dir = scratch("refresh");
    let path = write(&dir, "r.rs", "branch-b");
    let mut map = BTreeMap::new();
    let _prev = map.insert(description_key(&path, b"branch-b"), entry(&path, "B text", 1));
    let patches = refresh_with(&map, &[desc(&path, "A text", "deadbeef")]);
    assert_eq!(patches.len(), 1);
    let swap = patches.first().expect("one patch");
    assert_eq!(swap.description, "B text");
    assert_eq!(swap.old_hash, "deadbeef");
    // Fresh description: nothing to do.
    let fresh = digest(std::path::Path::new(&path)).expect("digest").file_hash;
    assert!(refresh_with(&map, &[desc(&path, "B text", &fresh)]).is_empty());
}

#[test]
fn apply_patches_skips_entries_that_moved_on() {
    let mut live = vec![desc("a", "old", "h1"), desc("b", "raced", "h9")];
    let patch = |path: &str| DescPatch {
        path: path.to_owned(),
        old_hash: "h1".to_owned(),
        description: "new".to_owned(),
        file_hash: "h2".to_owned(),
    };
    assert!(apply_patches(&mut live, vec![patch("a"), patch("b")]));
    assert_eq!(live, vec![desc("a", "new", "h2"), desc("b", "raced", "h9")]);
    assert!(!apply_patches(&mut live, vec![patch("b")]));
}

#[test]
fn populate_adds_fresh_skips_stale_and_existing() {
    let dir = scratch("populate");
    let fresh = write(&dir, "fresh.rs", "now");
    let stale = write(&dir, "stale.rs", "now");
    let known = write(&dir, "known.rs", "now");
    let mut map = BTreeMap::new();
    let _p1 = map.insert(description_key(&fresh, b"now"), entry(&fresh, "fresh d", 1));
    let _p2 = map.insert(description_key(&stale, b"before"), entry(&stale, "stale d", 1));
    let _p3 = map.insert(description_key(&known, b"now"), entry(&known, "yaml d", 1));
    let _p4 = map.insert("gone".to_owned(), entry(&dir.join("missing.rs").to_string_lossy(), "x", 1));
    let mut descriptions = vec![desc(&known, "memory d", "")];
    populate_with(&map, &mut descriptions);
    let paths: Vec<&str> = descriptions.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, vec![known.as_str(), fresh.as_str()]);
    assert_eq!(descriptions.get(1).map(|d| d.description.as_str()), Some("fresh d"));
}
