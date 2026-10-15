//! Owner-only, crash-safe file writes for secret material.
//!
//! Every secret file the vault persists (`.env`, the bridge cache, the bridge
//! token) goes through [`write_private_atomic`]: the bytes land in a sibling
//! temp file created `0600` from the first byte (no umask window), are
//! `fsync`ed, then `rename`d over the target. A crash leaves either the old
//! file or the new one, never a truncated one.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::types::VaultError;

/// Atomically replace `path` with `content`, owner read/write only.
///
/// # Errors
///
/// [`VaultError::Io`] if the parent directory, temp file, write, sync or
/// rename fails. The temp file is removed on failure.
pub(crate) fn write_private_atomic(path: &Path, content: &[u8]) -> Result<(), VaultError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| VaultError::Io(format!("cannot create {}: {e}", parent.display())))?;
    }
    let staging = temp_sibling(path);
    let result = write_and_rename(&staging, path, content);
    if result.is_err() {
        let _cleanup = fs::remove_file(&staging);
    }
    result
}

/// Write `content` to `staging` (created owner-only), sync, rename onto `path`.
fn write_and_rename(staging: &Path, path: &Path, content: &[u8]) -> Result<(), VaultError> {
    let mut file = private_options()
        .open(staging)
        .map_err(|e| VaultError::Io(format!("cannot create {}: {e}", staging.display())))?;
    file.write_all(content).map_err(|e| VaultError::Io(format!("write failed: {e}")))?;
    file.sync_all().map_err(|e| VaultError::Io(format!("fsync failed: {e}")))?;
    drop(file);
    fs::rename(staging, path).map_err(|e| VaultError::Io(format!("cannot replace {}: {e}", path.display())))
}

/// Per-process temp name next to `path`, so `rename` stays on one filesystem.
fn temp_sibling(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

/// `create_new` write options, mode `0600` on unix.
fn private_options() -> fs::OpenOptions {
    let mut opts = fs::OpenOptions::new();
    let _opts = opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        let _mode = opts.mode(0o600);
    }
    opts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_content_owner_only() {
        let dir = std::env::temp_dir().join(format!("cp-vault-securefs-{}", std::process::id()));
        let path = dir.join("secret.txt");
        write_private_atomic(&path, b"one").unwrap();
        write_private_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _cleanup = fs::remove_dir_all(&dir);
    }
}
