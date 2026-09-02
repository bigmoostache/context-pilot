//! One-time relocation of a pre-T713 oplog from the realm folder into the
//! global sync dir (`~/.context-pilot/sync/<id>/oplog`).
//!
//! Before T713 an agent's durable oplog lived at `<realm>/oplog`. With the sync
//! plane moved to `$HOME`, an existing agent must carry its oplog across on the
//! first boot after the upgrade — otherwise it starts a *fresh* log (command
//! dedup resets, and the message chokepoint could re-emit a `MessageCreated`
//! backlog). [`migrate_oplog`] performs that move exactly once, and is a no-op
//! for a brand-new agent (no old oplog) or one already migrated (new oplog
//! present).
//!
//! # Cross-device safety
//!
//! `rename(2)` is atomic and cheap only *within one filesystem*. With the global
//! location the realm (possibly on an external/removable drive or a different
//! mount) and `$HOME` can be on different devices, where `rename` fails with
//! `EXDEV`. So on any rename failure we fall back to a recursive **copy +
//! `fsync` + remove**: every file is copied and `fsync`'d, the destination tree
//! is `fsync`'d, and only then is the source removed — so a crash mid-migration
//! leaves the source intact (the move is re-attempted next boot) rather than a
//! half-moved, corrupt log.

use std::fs::{self, File};
use std::path::Path;

use crate::error::{BootResult, Error};

/// Move a pre-T713 `<realm>/oplog` to its new sync-dir location `new`, exactly
/// once. No-op when `new` already exists (already migrated) or `old` is absent
/// (a brand-new agent). Tries an atomic `rename` first, falling back to a
/// durable cross-device copy when the two paths straddle a filesystem boundary.
///
/// # Errors
///
/// Returns [`Error::Io`] if the relocation fails (rename error other than a
/// cross-device boundary, or a copy/fsync/remove failure in the fallback).
pub(super) fn migrate_oplog(old: &Path, new: &Path) -> BootResult<()> {
    if new.exists() || !old.exists() {
        return Ok(());
    }

    // Fast path: an atomic same-filesystem rename.
    if fs::rename(old, new).is_ok() {
        return Ok(());
    }

    // Fallback: the source and destination are on different devices (EXDEV) or
    // the rename otherwise failed — copy the whole tree durably, then remove the
    // source. Copying first (and only removing on success) keeps the move
    // crash-safe: an interrupted copy leaves the original oplog untouched.
    copy_tree_durable(old, new)
        .map_err(|e| Error::io(format!("copy oplog {} -> {}", old.display(), new.display()), e))?;
    fs::remove_dir_all(old).map_err(|e| Error::io(format!("remove migrated oplog {}", old.display()), e))?;
    Ok(())
}

/// Recursively copy directory `src` to `dst`, `fsync`ing every copied file and
/// each created directory so the relocated tree is durable before the caller
/// removes the source.
fn copy_tree_durable(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for raw in fs::read_dir(src)? {
        let dirent = raw?;
        let from = dirent.path();
        let to = dst.join(dirent.file_name());
        if dirent.file_type()?.is_dir() {
            copy_tree_durable(&from, &to)?;
        } else {
            let _copied = fs::copy(&from, &to)?;
            File::open(&to)?.sync_all()?;
        }
    }
    // fsync the directory so its new entries survive a crash.
    File::open(dst)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn noop_when_new_exists() {
        let dir = tempdir().expect("dir");
        let old = dir.path().join("old-oplog");
        let new = dir.path().join("new-oplog");
        fs::create_dir_all(&old).expect("mk old");
        fs::write(old.join("seg"), b"OLD").expect("seed old");
        fs::create_dir_all(&new).expect("mk new");
        fs::write(new.join("seg"), b"NEW").expect("seed new");

        migrate_oplog(&old, &new).expect("migrate");
        // The already-present new oplog is untouched; the old one is left alone.
        assert_eq!(fs::read(new.join("seg")).expect("read new"), b"NEW");
        assert!(old.exists(), "old is not removed when new already exists");
    }

    #[test]
    fn noop_when_old_absent() {
        let dir = tempdir().expect("dir");
        let old = dir.path().join("old-oplog");
        let new = dir.path().join("new-oplog");
        migrate_oplog(&old, &new).expect("migrate");
        assert!(!new.exists(), "nothing is created when there is no old oplog");
    }

    #[test]
    fn moves_tree_with_bodies_and_removes_source() {
        let dir = tempdir().expect("dir");
        let old = dir.path().join("old-oplog");
        let new = dir.path().join("sync").join("id").join("oplog");
        fs::create_dir_all(old.join("bodies")).expect("mk old/bodies");
        fs::write(old.join("segment-0"), b"seg-data").expect("seed seg");
        fs::write(old.join("bodies").join("ab12"), b"body-data").expect("seed body");

        migrate_oplog(&old, &new).expect("migrate");

        assert!(!old.exists(), "source oplog is removed after migration");
        assert_eq!(fs::read(new.join("segment-0")).expect("read seg"), b"seg-data");
        assert_eq!(fs::read(new.join("bodies").join("ab12")).expect("read body"), b"body-data");
    }
}
