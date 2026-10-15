//! Memoized per-file content digests.
//!
//! The tree reads described files for two hashes: the 8-hex `file_hash` that
//! drives the `[!]` stale marker, and the 16-hex YAML key over
//! `path ∥ content`. Both come from one read here, and the result is reused
//! until the file's `(mtime, len)` changes, so an unchanged file costs one
//! `stat` per tree refresh instead of one or two full reads.
//!
//! The two widths differ on purpose: `file_hash` is persisted in
//! `modules/tree.json` at 8 chars, and widening it would mark every
//! existing description stale.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

/// Both hashes of one file's current content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileDigest {
    /// First 8 hex chars of FNV-1a(content): the `[!]` staleness stamp.
    pub file_hash: String,
    /// First 16 hex chars of FNV-1a(path ∥ content): the YAML key.
    pub desc_key: String,
}

/// `(mtime, len)` a cached digest was computed at.
type Stamp = (Option<SystemTime>, u64);

/// Digests by path (as given), each with the stamp it was computed at.
type DigestCache = HashMap<PathBuf, (Stamp, FileDigest)>;

/// Process-wide digest cache.
static CACHE: LazyLock<Mutex<DigestCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// YAML key for `path` holding `content`: 16 hex chars of FNV-1a(path ∥ content).
///
/// The path is part of the key, so a copy or rename gets its own entry.
pub(crate) fn description_key(path: &str, content: &[u8]) -> String {
    let mut data = Vec::with_capacity(path.len().saturating_add(content.len()));
    data.extend_from_slice(path.as_bytes());
    data.extend_from_slice(content);
    let hex = cp_mod_utilities::hash::compute(&data);
    hex.get(..16).unwrap_or(&hex).to_owned()
}

/// Digest of the file at `path`; `None` if it is missing, a directory or unreadable.
///
/// Reads the file only when its `(mtime, len)` differs from the cached stamp.
pub(crate) fn digest(path: &Path) -> Option<FileDigest> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let stamp = (meta.modified().ok(), meta.len());
    let hit = CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(path)
        .filter(|cached| cached.0 == stamp)
        .map(|cached| cached.1.clone());
    if hit.is_some() {
        return hit;
    }
    let content = std::fs::read(path).ok()?;
    let hex = cp_mod_utilities::hash::compute(&content);
    let fresh = FileDigest {
        file_hash: hex.get(..8).unwrap_or(&hex).to_owned(),
        desc_key: description_key(&path.to_string_lossy(), &content),
    };
    let _prev = CACHE.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_path_buf(), (stamp, fresh.clone()));
    Some(fresh)
}
