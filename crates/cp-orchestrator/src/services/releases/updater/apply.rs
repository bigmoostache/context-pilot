//! Atomic apply — both binaries move together, the database is backed up, and
//! nothing is promoted until the new binary proves healthy (§5.5 steps 2-6).
//!
//! Lifecycle of one apply, and who owns each transition:
//!
//! 1. [`stage_apply`] (running orchestrator): back up `auth.db`, record the
//!    in-flight update in `releases/pending-update.json`, stage the new
//!    `cp-orchestrator` over the install path (atomic rename + `.pending` /
//!    `.bak` markers from [`stage_orchestrator_update`]). `active_tag` and the
//!    agent (`tui`) binary are **not** touched yet.
//! 2. [`restart_self`] re-execs the install path — the new binary takes over
//!    this PID.
//! 3. Healthy boot: the health-gated committer (`boot_commit_when_healthy`)
//!    clears the binary markers, then [`promote_committed`] flips
//!    `active_tag`/agent binary to the new tag (both binaries now point at the
//!    same release — §5.5 step 5), drops the DB backup, records `success`.
//! 4. Crash-looping boot: `boot_check` restores the `.bak` binary after the
//!    tolerance; on the old binary's next boot [`boot_reconcile`] sees the
//!    orphaned `pending-update.json`, **restores `auth.db`** from the backup
//!    (a forward migration may have run — §5.8), and records `rolled_back`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::{ReleaseStore, self_update, stage_orchestrator_update};
use super::state::{UpdateResult, UpdateState, now_ms};
use crate::services::auth::store::AuthStore;

/// In-flight update record under the releases directory.
const PENDING_UPDATE_FILE: &str = "pending-update.json";

/// What was in flight when the restart was triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingUpdate {
    /// Tag active before the apply (`None` on a first install).
    from: Option<String>,
    /// Tag being applied.
    to: String,
    /// Where `auth.db` was backed up, if a database existed.
    db_backup: Option<PathBuf>,
}

/// Path of the in-flight record under `releases_dir`.
fn pending_update_path(releases_dir: &Path) -> PathBuf {
    releases_dir.join(PENDING_UPDATE_FILE)
}

/// Stage an update to `to_tag` (already downloaded + sha-verified): back up
/// the database, record the in-flight update, and swap the orchestrator
/// binary on the install path. The caller triggers the restart.
///
/// # Errors
///
/// Returns an error (with the in-flight record cleaned up) if the release
/// does not ship both binaries, the backup fails, or staging fails. The
/// install path is left as it was on any error.
pub fn stage_apply(
    store: &ReleaseStore,
    auth: Option<&AuthStore>,
    auth_db_path: &Path,
    install: &Path,
    to_tag: &str,
) -> Result<(), String> {
    // Both binaries must ship in the release — they move together (§5.1).
    let new_orchestrator = store.orchestrator_binary_path(to_tag);
    if !new_orchestrator.exists() {
        return Err(format!("release {to_tag} ships no cp-orchestrator binary"));
    }
    if !store.binary_path(to_tag).exists() {
        return Err(format!("release {to_tag} ships no cpilot binary"));
    }

    let from = store.active_tag().map(str::to_owned);

    // 1. Back up auth.db BEFORE anything moves (§5.5 step 2, §5.8). The
    //    SQLite online-backup API gives a consistent snapshot while the store
    //    is live; a bare file copy covers the store-less (auth off) case.
    let db_backup = if auth_db_path.exists() {
        let backup = db_backup_path(auth_db_path, from.as_deref());
        match auth {
            Some(auth) => auth.backup_to(&backup).map_err(|e| format!("auth.db backup: {e}"))?,
            None => {
                let _bytes = std::fs::copy(auth_db_path, &backup).map_err(|e| format!("auth.db backup: {e}"))?;
            }
        }
        Some(backup)
    } else {
        None
    };

    // 2. Record the in-flight update BEFORE staging: if we crash in between,
    //    boot_reconcile treats it as a (harmless) rollback and cleans up.
    let pending = PendingUpdate { from, to: to_tag.to_owned(), db_backup };
    let bytes = serde_json::to_vec_pretty(&pending).map_err(|e| format!("serialize pending-update: {e}"))?;
    let path = pending_update_path(store.dir());
    std::fs::write(&path, &bytes).map_err(|e| format!("write pending-update: {e}"))?;

    // 3. Swap the orchestrator binary (atomic rename + rollback markers).
    if let Err(e) = stage_orchestrator_update(install, &new_orchestrator) {
        let _rm = std::fs::remove_file(&path);
        return Err(format!("staging orchestrator {to_tag}: {e}"));
    }
    Ok(())
}

/// Promote a committed update: flip `active_tag` + the agent binary to the
/// staged tag, drop the DB backup, record `success`. Call **only after** the
/// health-gated boot commit blessed the new binary.
///
/// Returns the new agent binary path when an update was promoted, `None` when
/// nothing was in flight (plain restart).
///
/// # Errors
///
/// Returns an error if the in-flight record is unreadable or the release
/// vanished from disk; the record is preserved for inspection.
pub fn promote_committed(store: &mut ReleaseStore, _auth_db_path: &Path) -> Result<Option<PathBuf>, String> {
    let path = pending_update_path(store.dir());
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(None); // nothing in flight — a normal boot
    };
    let pending: PendingUpdate = serde_json::from_slice(&bytes).map_err(|e| format!("pending-update parse: {e}"))?;

    // Both binaries move together (§5.5 step 5): the running orchestrator is
    // already the new tag; select() repoints the agent binary + active_tag.
    let agent_binary = store.select(&pending.to)?;

    if let Some(backup) = &pending.db_backup {
        let _rm = std::fs::remove_file(backup);
    }
    let _rm = std::fs::remove_file(&path);

    let mut st = UpdateState::load(store.dir());
    st.available = None;
    st.last_result = Some(UpdateResult::Success { from: pending.from, to: pending.to, at_ms: now_ms() });
    st.save(store.dir());
    Ok(Some(agent_binary))
}

/// Boot-time reconciliation of a **failed** apply. Call early in `main`,
/// after `boot_check` and **before** the auth store opens.
///
/// No-op unless an in-flight record exists with no `.pending` boot marker —
/// exactly the signature of "`boot_check` rolled the binary back last boot".
/// Then: restore `auth.db` from the backup (a forward migration may have run,
/// §5.8), clear the record, and record `rolled_back`.
pub fn boot_reconcile(releases_dir: &Path, auth_db_path: &Path, install: &Path) {
    let path = pending_update_path(releases_dir);
    let Ok(bytes) = std::fs::read(&path) else {
        return; // nothing in flight
    };
    if self_update::pending_path(install).exists() {
        return; // apply still in flight — this boot is one of its attempts
    }
    let Ok(pending) = serde_json::from_slice::<PendingUpdate>(&bytes) else {
        let _rm = std::fs::remove_file(&path);
        return;
    };

    // The staged binary was rolled back — restore the matching database.
    if let Some(backup) = &pending.db_backup {
        if backup.exists() {
            match std::fs::copy(backup, auth_db_path) {
                Ok(_bytes) => {
                    // Stale WAL/SHM would shadow the restored file's content.
                    for suffix in ["-wal", "-shm"] {
                        let mut os = auth_db_path.as_os_str().to_owned();
                        os.push(suffix);
                        let _rm = std::fs::remove_file(PathBuf::from(os));
                    }
                    let _rm = std::fs::remove_file(backup);
                    eprintln!("updater: rollback — auth.db restored from {}", backup.display());
                }
                Err(e) => eprintln!("updater: rollback db restore FAILED ({e}) — backup kept at {}", backup.display()),
            }
        }
    }
    let _rm = std::fs::remove_file(&path);

    let mut st = UpdateState::load(releases_dir);
    st.last_result =
        Some(UpdateResult::RolledBack { to: pending.from.clone(), attempted: pending.to.clone(), at_ms: now_ms() });
    st.save(releases_dir);
    eprintln!(
        "updater: update to {} failed — rolled back to {}",
        pending.to,
        pending.from.as_deref().unwrap_or("(previous)")
    );
}

/// Re-exec the install path after a short delay (lets the HTTP reply flush).
/// The process image is replaced on the same PID — no restart-burst consumed;
/// falls back to `SIGTERM` so a supervisor can respawn us if `exec` fails.
pub fn restart_self() {
    use std::os::unix::process::CommandExt as _;
    let install = std::env::current_exe().ok();
    let _restart = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Some(exe) = install {
            let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
            let err = std::process::Command::new(&exe).args(&args).exec();
            eprintln!("updater: exec of {} failed: {err}; falling back to SIGTERM", exe.display());
        } else {
            eprintln!("updater: current_exe() unavailable; falling back to SIGTERM");
        }
        let _sent = nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGTERM);
    });
}

/// Sibling backup path: `auth.db.bak-<oldtag>` (`§5.5` step 2).
fn db_backup_path(auth_db_path: &Path, from: Option<&str>) -> PathBuf {
    let mut name = auth_db_path.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    name.push(format!(".bak-{}", from.unwrap_or("none")));
    auth_db_path.with_file_name(name)
}
