//! Durable **`provisioned`** flag — the boot gate for the product cockpit.
//!
//! A fresh appliance boots *unprovisioned*: the cockpit is served over cleartext
//! `:80` (day-0) and an IT operator names the box in the cockpit, which flips
//! this flag to *provisioned* — at which point Caddy is reconfigured to serve the
//! cockpit on `:443` (design §13.4). The flag is a single small file so it
//! survives reboots with no database dependency and can be inspected/repaired by
//! hand on the box.
//!
//! This is deliberately **distinct from** the product's `onboarding_completed`
//! UI setting (`transport/rest/config/settings.rs`): that flag is a cosmetic
//! "has the user seen the welcome flow" marker, whereas this one is the security
//! gate that decides whether the cockpit is served at all.

use std::io::Write as _;
use std::path::Path;

/// Whether the box has been provisioned (the flag file exists and reads `true`).
///
/// Any read error (missing file, unreadable) is treated as **not provisioned** —
/// fail-closed, so a damaged or absent flag keeps the cockpit gated rather than
/// exposing it by default.
pub(crate) fn is_provisioned(flag_path: &Path) -> bool {
    std::fs::read_to_string(flag_path).map(|s| s.trim() == "true").unwrap_or(false)
}

/// Persist the `provisioned` flag **atomically and durably**.
///
/// Write-tmp → `fsync` the file → rename → `fsync` the parent directory. The
/// rename gives concurrent readers an all-or-nothing view (never a torn write),
/// and the two `fsync`s make the change survive an abrupt power loss — important
/// on the OpenWrt appliance, where a finalize the operator was told succeeded
/// must not silently revert after a yank of the power. (ext4's delayed-allocation
/// flush heuristic does not reliably cover a rename onto a *new* name, which is
/// exactly the first-ever finalize, so we fsync explicitly rather than rely on
/// it.) The directory fsync is best-effort: on the rare filesystem that refuses
/// to open a dir for sync we keep the (already-renamed) result instead of failing.
///
/// Creates the parent directory if needed. `true` writes the flag; `false`
/// rewrites it to the unprovisioned value (tests + a future de-provision path)
/// rather than deleting, so the file's presence is stable.
///
/// # Errors
///
/// Returns the underlying I/O error if the directory cannot be created or the
/// file cannot be written, synced, or renamed.
pub(crate) fn set_provisioned(flag_path: &Path, value: bool) -> std::io::Result<()> {
    write_atomic(flag_path, if value { b"true\n" } else { b"false\n" })
}

/// Atomically and durably write `bytes` to `path`: create the parent dir,
/// write-tmp → `fsync` file → rename → `fsync` parent dir. Shared by the
/// maintenance plane's durable state (provisioned flag, identity, Caddyfile) so
/// every on-disk mutation has the same crash-safety guarantees. See
/// [`set_provisioned`] for the rationale on the explicit fsyncs.
///
/// # Errors
///
/// Returns the underlying I/O error if the directory cannot be created or the
/// file cannot be written, synced, or renamed.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _synced = dir.sync_all(); // best-effort: persist the rename in the dir entry
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_round_trips_and_defaults_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let flag = dir.path().join("orchestrator").join(".provisioned");

        // Absent flag → not provisioned (fail-closed).
        assert!(!is_provisioned(&flag), "a missing flag reads as unprovisioned");

        // Set true → reads provisioned; the parent dir was created.
        set_provisioned(&flag, true).expect("write true");
        assert!(is_provisioned(&flag), "after finalize the flag reads provisioned");

        // Idempotent re-write keeps it provisioned.
        set_provisioned(&flag, true).expect("re-write true");
        assert!(is_provisioned(&flag), "re-finalize is idempotent");

        // Set false → not provisioned, file still present (no temp left behind).
        set_provisioned(&flag, false).expect("write false");
        assert!(!is_provisioned(&flag));
        assert!(!flag.with_extension("tmp").exists(), "no temp file is left behind");
    }
}
