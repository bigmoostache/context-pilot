//! Orchestrator self-update (adopt a downloaded `cp-orchestrator`).
//!
//! The "Update & Restart Orchestrator" button stages a freshly downloaded
//! `cp-orchestrator` over the running install path and then re-execs it. Because
//! you cannot overwrite a running executable in place (`ETXTBSY`), we write the
//! new bytes to a sibling temp file and `rename` it over the install path — that
//! atomically swaps the directory entry to the new inode while the running
//! process keeps its old (now-unlinked) inode until it re-execs.
//!
//! Safety: before swapping we back the current binary up to `<name>.bak`, and we
//! drop a `<name>.pending` marker holding a boot-attempt counter. On startup
//! [`boot_check`] increments that counter; if a staged update fails to boot
//! [`MAX_BOOT_ATTEMPTS`] times (crash-loop under a supervisor), it automatically
//! restores the `.bak`, so a bad update self-heals instead of bricking the box.
//! A healthy boot calls [`boot_commit`] to clear the marker and backup.

use std::path::PathBuf;

/// How many failed boot attempts of a staged update we tolerate before the
/// startup guard rolls back to the `.bak` binary.
const MAX_BOOT_ATTEMPTS: u32 = 2;

/// Sibling path for the backup of the previous orchestrator binary.
pub(crate) fn backup_path(install: &std::path::Path) -> PathBuf {
    with_suffix(install, "bak")
}

/// Sibling path for the boot-attempt marker of a staged update.
pub(crate) fn pending_path(install: &std::path::Path) -> PathBuf {
    with_suffix(install, "pending")
}

/// Append a `.suffix` to a path's file name (not `with_extension`, which would
/// clobber an existing extension — the binary has none, but be explicit).
fn with_suffix(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

/// Stage `src` (a downloaded `cp-orchestrator`) over the running `install`
/// binary via atomic rename, backing the current binary up to `<name>.bak` and
/// writing a fresh `<name>.pending` boot-attempt marker.
///
/// The running process is untouched (it keeps its open inode); the swap only
/// takes effect when the process re-execs the install path.
///
/// # Errors
///
/// Returns an error if `src` is missing/empty or any filesystem step fails. On
/// error the install path is left as it was (best-effort — the backup copy is
/// non-destructive).
pub fn stage_orchestrator_update(install: &std::path::Path, src: &std::path::Path) -> Result<(), String> {
    let meta = std::fs::metadata(src).map_err(|e| format!("stat {}: {e}", src.display()))?;
    if meta.len() == 0 {
        return Err(format!("{} is empty", src.display()));
    }

    // 1. Back up the current binary (copy, so `install` is never absent).
    let bak = backup_path(install);
    let _bytes =
        std::fs::copy(install, &bak).map_err(|e| format!("backup {} -> {}: {e}", install.display(), bak.display()))?;

    // 2. Write the new bytes to a sibling temp and make it executable.
    let staged = with_suffix(install, "new");
    let _bytes =
        std::fs::copy(src, &staged).map_err(|e| format!("stage {} -> {}: {e}", src.display(), staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _r = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }

    // 3. Atomically swap the new binary into place (dodges ETXTBSY).
    std::fs::rename(&staged, install).map_err(|e| {
        let _cleanup = std::fs::remove_file(&staged);
        format!("promote {} -> {}: {e}", staged.display(), install.display())
    })?;

    // 4. Drop the boot-attempt marker (counter starts at 0).
    let _w = std::fs::write(pending_path(install), b"0");
    Ok(())
}

/// Startup guard: account for a staged update's boot attempt.
///
/// If a `.pending` marker exists, increment its counter. Once the counter
/// reaches [`MAX_BOOT_ATTEMPTS`] (the staged binary keeps crashing on boot),
/// restore the `.bak` binary over the install path and clear the markers so the
/// service self-heals back to the last-known-good binary. Call this **before**
/// binding, as early in `main` as possible.
pub fn boot_check(install: &std::path::Path) {
    let pending = pending_path(install);
    let Ok(raw) = std::fs::read_to_string(&pending) else {
        return; // No staged update in flight.
    };
    let attempts: u32 = raw.trim().parse::<u32>().unwrap_or(0).saturating_add(1);

    if attempts >= MAX_BOOT_ATTEMPTS {
        // The staged update is crash-looping — roll back to the backup.
        let bak = backup_path(install);
        if bak.exists() {
            if let Err(e) = std::fs::rename(&bak, install) {
                eprintln!("self-update: rollback {} -> {} failed: {e}", bak.display(), install.display());
            } else {
                eprintln!(
                    "self-update: staged orchestrator failed to boot {attempts}× — rolled back to previous binary"
                );
            }
        }
        let _rm = std::fs::remove_file(&pending);
    } else {
        // Still within the tolerance window — record this attempt.
        let _w = std::fs::write(&pending, attempts.to_string().as_bytes());
    }
}

/// Commit a staged update after a healthy boot: clear the `.pending` marker and
/// delete the `.bak` backup. Call once the process is known to be running
/// normally (e.g. after it has stayed up past a short grace period).
pub fn boot_commit(install: &std::path::Path) {
    let pending = pending_path(install);
    if !pending.exists() {
        return; // Nothing staged; normal boot.
    }
    let _rm_pending = std::fs::remove_file(&pending);
    let _rm_bak = std::fs::remove_file(backup_path(install));
    eprintln!("self-update: orchestrator update committed (previous binary backup removed)");
}
