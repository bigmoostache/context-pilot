//! Atomic, `0600` write of the agent registry record (design doc §10).
//!
//! The registry file `~/.context-pilot/agents/<id>.json` is the single
//! discovery artifact the backend watches. Two properties matter:
//!
//! * **Atomicity** — a reader (the backend's directory watcher) must never see
//!   a half-written record. We write to a sibling `*.tmp`, `fsync` it, then
//!   `rename` it into place (rename is atomic within a directory), and finally
//!   `fsync` the directory so the new entry survives a crash.
//! * **Confidentiality** — the record carries the bearer [`cap_token`], so the
//!   file is created `0600` (owner read/write only) from the very first byte:
//!   the `tmp` file is opened with the restrictive mode, never widened.
//!
//! [`cap_token`]: cp_wire::types::registry::Entry::cap_token

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use cp_wire::types::registry::Entry;

use crate::error::{BootResult, Error};

/// Restrictive mode for the registry file: owner read+write only (`0600`).
/// The file embeds the capability token, so it must never be group/world
/// readable.
const REGISTRY_MODE: u32 = 0o600;

/// The default agents directory under the user's home: `~/.context-pilot/agents`.
///
/// # Errors
///
/// Returns [`Error::Io`] if `$HOME` is unset (there is nowhere to anchor
/// the path).
pub fn default_agents_dir() -> BootResult<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::io("resolve agents dir", std::io::Error::other("$HOME is not set")))?;
    Ok(PathBuf::from(home).join(".context-pilot").join("agents"))
}

/// The path of the registry file for agent `id` inside `agents_dir`.
#[must_use]
pub fn path(agents_dir: &Path, id: &str) -> PathBuf {
    agents_dir.join(format!("{id}.json"))
}

/// Write `entry` atomically to `agents_dir/<id>.json` with `0600` permissions,
/// creating `agents_dir` if it does not exist. Returns the final path.
///
/// The write is `tmp → fsync → rename → fsync(dir)`, so a crash leaves either
/// the old record or the new one, never a torn file, and the rename is durable.
///
/// # Errors
///
/// Returns [`Error::Io`] if the directory cannot be created, the temporary
/// file cannot be written or `fsync`'d, the rename fails, or the entry cannot
/// be serialised.
pub fn write_entry(agents_dir: &Path, entry: &Entry) -> BootResult<PathBuf> {
    fs::create_dir_all(agents_dir).map_err(|e| Error::io(format!("create agents dir {}", agents_dir.display()), e))?;

    let json = serde_json::to_vec_pretty(entry)
        .map_err(|e| Error::io("serialise registry entry", std::io::Error::other(e)))?;

    let final_path = path(agents_dir, &entry.id);
    let tmp_path = agents_dir.join(format!("{}.json.tmp", entry.id));

    write_tmp(&tmp_path, &json)?;

    fs::rename(&tmp_path, &final_path).map_err(|e| {
        // Leave no stale tmp behind on a failed rename.
        let _ignored = fs::remove_file(&tmp_path);
        Error::io(format!("rename {} into place", tmp_path.display()), e)
    })?;

    sync_dir(agents_dir)?;
    Ok(final_path)
}

/// Create the `0600` temp file, write `bytes`, and `fdatasync` it.
fn write_tmp(tmp_path: &Path, bytes: &[u8]) -> BootResult<()> {
    let mut file: File = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(REGISTRY_MODE)
        .open(tmp_path)
        .map_err(|e| Error::io(format!("open temp registry {}", tmp_path.display()), e))?;
    file.write_all(bytes).map_err(|e| Error::io(format!("write temp registry {}", tmp_path.display()), e))?;
    file.sync_all().map_err(|e| Error::io(format!("fsync temp registry {}", tmp_path.display()), e))?;
    Ok(())
}

/// `fsync` a directory so a freshly `rename`d child entry is durable.
fn sync_dir(dir: &Path) -> BootResult<()> {
    let handle = File::open(dir).map_err(|e| Error::io(format!("open dir {}", dir.display()), e))?;
    handle.sync_all().map_err(|e| Error::io(format!("fsync dir {}", dir.display()), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::types::registry::AgentStatus;
    use std::os::unix::fs::PermissionsExt as _;
    use tempfile::tempdir;

    fn sample(id: &str) -> Entry {
        Entry {
            schema_version: 1,
            id: id.to_owned(),
            folder: "/proj".to_owned(),
            pid: 42,
            boot_id: "boot".to_owned(),
            model: "m".to_owned(),
            protocol_version: 1,
            binary_version: "0.1.0".to_owned(),
            socket_path: "/proj/.context-pilot/stream.sock".to_owned(),
            oplog_path: "/proj/.context-pilot/oplog".to_owned(),
            heartbeat_path: "/proj/.context-pilot/heartbeat".to_owned(),
            cap_token: "secret".to_owned(),
            started_at_ms: 0,
            status: AgentStatus::Starting,
        }
    }

    #[test]
    fn write_entry_is_readable_and_round_trips() {
        let dir = tempdir().expect("tempdir");
        let path = write_entry(dir.path(), &sample("abc")).expect("write");
        assert_eq!(path, dir.path().join("abc.json"));
        let text = fs::read_to_string(&path).expect("read back");
        let back: Entry = serde_json::from_str(&text).expect("parse");
        assert_eq!(back.id, "abc");
        assert_eq!(back.status, AgentStatus::Starting);
    }

    #[test]
    fn write_entry_is_mode_0600() {
        let dir = tempdir().expect("tempdir");
        let path = write_entry(dir.path(), &sample("perm")).expect("write");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "registry must be owner-only");
    }

    #[test]
    fn write_entry_leaves_no_tmp() {
        let dir = tempdir().expect("tempdir");
        let _path = write_entry(dir.path(), &sample("clean")).expect("write");
        let tmp = dir.path().join("clean.json.tmp");
        assert!(!tmp.exists(), "the temp file must be renamed away");
    }

    #[test]
    fn write_entry_creates_missing_dir() {
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("a").join("b").join("agents");
        let path = write_entry(&nested, &sample("nested")).expect("write");
        assert!(path.exists(), "missing agents dir is created");
    }

    #[test]
    fn write_entry_overwrites_previous() {
        let dir = tempdir().expect("tempdir");
        let _first = write_entry(dir.path(), &sample("same")).expect("first");
        let mut updated = sample("same");
        updated.status = AgentStatus::Running;
        let path = write_entry(dir.path(), &updated).expect("second");
        let back: Entry = serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("parse");
        assert_eq!(back.status, AgentStatus::Running, "rename replaces the old record atomically");
    }
}
