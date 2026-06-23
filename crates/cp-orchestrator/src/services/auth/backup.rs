//! **Auth database backup** — rolling + daily permanent snapshots (NFR-19/20).
//!
//! [`BackupScheduler`] is driven by the runtime's slow-cadence loop. On each
//! tick it checks whether enough time has elapsed since the last rolling
//! backup (~5 min) or whether a daily snapshot slot (AM / PM) is unfilled,
//! and performs the backup via the SQLite online backup API (consistent,
//! lock-free reads).
//!
//! File layout (all siblings of the auth database):
//!
//! ```text
//! ~/.context-pilot/orchestrator/
//! ├── auth.db                          ← live database
//! ├── auth.db.rolling                  ← overwritten every ~5 min
//! └── backups/
//!     ├── auth-2026-06-23-am.db        ← permanent, one per AM
//!     └── auth-2026-06-23-pm.db        ← permanent, one per PM
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::store::AuthStore;
use super::types::AuthError;

/// Interval between rolling backups (overwrite the single rolling file).
const ROLLING_INTERVAL_MS: u64 = 5 * 60 * 1000; // 5 minutes

/// Tracks backup timing so the runtime loop can call [`tick`](Self::tick)
/// cheaply on every slow-cadence iteration (~2 s) without doing redundant
/// work.
#[derive(Debug)]
pub(crate) struct BackupScheduler {
    /// Absolute path of the live auth database (needed to derive sibling
    /// backup paths).
    db_path: PathBuf,
    /// Epoch-ms of the last successful rolling backup.
    last_rolling_ms: u64,
    /// `"YYYY-MM-DD-am"` or `"YYYY-MM-DD-pm"` tag of the last daily
    /// snapshot written, so we create at most one per half-day.
    last_daily_tag: String,
}

impl BackupScheduler {
    /// Create a scheduler for the database at `db_path`.
    ///
    /// The first rolling backup fires on the first tick (no initial delay).
    pub(crate) fn new(db_path: PathBuf) -> Self {
        Self {
            db_path,
            last_rolling_ms: 0,
            last_daily_tag: String::new(),
        }
    }

    /// Check whether a rolling or daily backup is due and perform it.
    ///
    /// Designed to be called frequently (every ~2 s); internally gates on
    /// elapsed time so the actual I/O is rare.
    pub(crate) fn tick(&mut self, auth: &AuthStore) {
        let now = super::helpers::now_ms();

        // ── Rolling backup ──────────────────────────────────────────
        if now.saturating_sub(self.last_rolling_ms) >= ROLLING_INTERVAL_MS {
            let dest = self.rolling_path();
            match auth.backup_to(&dest) {
                Ok(()) => {
                    self.last_rolling_ms = now;
                    eprintln!("auth backup: rolling snapshot → {}", dest.display());
                }
                Err(err) => {
                    eprintln!("WARN: auth rolling backup failed: {err}");
                }
            }
        }

        // ── Daily permanent snapshot (AM / PM) ──────────────────────
        let tag = Self::daily_tag(now);
        if tag != self.last_daily_tag {
            let dest = self.daily_path(&tag);
            // Only create if the file does not already exist (idempotent
            // across process restarts within the same half-day).
            if !dest.exists() {
                if let Some(parent) = dest.parent() {
                    let _created = std::fs::create_dir_all(parent);
                }
                match auth.backup_to(&dest) {
                    Ok(()) => {
                        eprintln!("auth backup: daily snapshot → {}", dest.display());
                    }
                    Err(err) => {
                        eprintln!("WARN: auth daily backup failed: {err}");
                    }
                }
            }
            self.last_daily_tag = tag;
        }
    }

    // ── Path helpers ────────────────────────────────────────────────

    /// `<dir>/auth.db.rolling`
    fn rolling_path(&self) -> PathBuf {
        let mut p = self.db_path.clone();
        let name = format!(
            "{}.rolling",
            p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
        );
        p.set_file_name(name);
        p
    }

    /// `<dir>/backups/auth-YYYY-MM-DD-{am,pm}.db`
    fn daily_path(&self, tag: &str) -> PathBuf {
        let parent = self.db_path.parent().unwrap_or_else(|| Path::new("."));
        parent.join("backups").join(format!("auth-{tag}.db"))
    }

    /// Produce a `"YYYY-MM-DD-am"` or `"YYYY-MM-DD-pm"` tag from epoch-ms.
    fn daily_tag(epoch_ms: u64) -> String {
        // Convert to seconds and derive UTC date components.
        let secs = epoch_ms / 1000;
        let (year, month, day, hour) = epoch_to_ymd_h(secs);
        let half = if hour < 12 { "am" } else { "pm" };
        format!("{year:04}-{month:02}-{day:02}-{half}")
    }
}

// ─────────────── AuthStore backup method ─────────────────────────────

impl AuthStore {
    /// Create a consistent backup of the database to `dest` using the SQLite
    /// online backup API.
    ///
    /// Safe to call while other threads read the same connection (WAL mode).
    /// The backup is atomic — `dest` is a complete, self-contained database
    /// on success.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] if the destination cannot be opened or
    /// the backup fails.
    pub(crate) fn backup_to(&self, dest: &Path) -> Result<(), AuthError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|_io_err| {
                AuthError::Database(rusqlite::Error::InvalidPath(
                    parent.to_path_buf().into(),
                ))
            })?;
        }
        let mut dst = rusqlite::Connection::open(dest)?;
        let backup = rusqlite::backup::Backup::new(&self.conn, &mut dst)?;
        // 100 pages per step, no pause — our auth.db is tiny.
        backup.run_to_completion(100, Duration::from_millis(0), None)?;
        Ok(())
    }
}

// ─────────────── Minimal UTC date extraction ─────────────────────────

/// Convert seconds-since-epoch to `(year, month, day, hour)` in UTC.
///
/// Civil-time algorithm from Howard Hinnant (public domain). No `chrono`
/// dependency — the auth backup is the only consumer and only needs the
/// date + hour.
fn epoch_to_ymd_h(secs: u64) -> (i32, u32, u32, u32) {
    let hour = ((secs % 86400) / 3600) as u32;

    // Days since 0000-03-01 (era-based algorithm).
    let z = (secs / 86400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32; // day-of-era  [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (i32::try_from(y).unwrap_or(9999), m, d, hour)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path as StdPath;

    #[test]
    fn epoch_to_ymd_h_known_dates() {
        // 2026-06-23 14:30:00 UTC = 1782225000
        let (y, m, d, h) = epoch_to_ymd_h(1_782_225_000);
        assert_eq!((y, m, d), (2026, 6, 23));
        assert_eq!(h, 14);

        // Unix epoch = 1970-01-01 00:00:00
        let (y, m, d, h) = epoch_to_ymd_h(0);
        assert_eq!((y, m, d, h), (1970, 1, 1, 0));
    }

    #[test]
    fn daily_tag_am_pm() {
        // 2026-06-23 03:00:00 UTC → AM
        let tag = BackupScheduler::daily_tag(1_782_210_000_000);
        assert!(tag.ends_with("-am"), "tag={tag}");

        // 2026-06-23 15:00:00 UTC → PM
        let tag = BackupScheduler::daily_tag(1_782_253_200_000);
        assert!(tag.ends_with("-pm"), "tag={tag}");
    }

    #[test]
    fn rolling_path_is_sibling() {
        let sched = BackupScheduler::new(PathBuf::from("/tmp/orch/auth.db"));
        let rp = sched.rolling_path();
        assert_eq!(rp, PathBuf::from("/tmp/orch/auth.db.rolling"));
    }

    #[test]
    fn daily_path_in_backups_subdir() {
        let sched = BackupScheduler::new(PathBuf::from("/tmp/orch/auth.db"));
        let dp = sched.daily_path("2026-06-23-pm");
        assert_eq!(dp, PathBuf::from("/tmp/orch/backups/auth-2026-06-23-pm.db"));
    }

    #[test]
    fn backup_to_creates_consistent_copy() {
        let store =
            AuthStore::open(StdPath::new(":memory:")).expect("open in-memory");
        // Seed a user so the backup is non-trivial.
        let _user = store
            .create_user("backup@test.com", "Bak", "password1234", super::super::types::UserRole::User)
            .expect("create user");

        let tmp = std::env::temp_dir().join("cp-auth-backup-test.db");
        // Clean up from any previous run.
        let _removed = std::fs::remove_file(&tmp);

        store.backup_to(&tmp).expect("backup_to");

        // The backup file should exist and be a valid SQLite database.
        assert!(tmp.exists(), "backup file should exist");
        let restored = rusqlite::Connection::open(&tmp).expect("open backup");
        let count: i64 = restored
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .expect("query");
        assert_eq!(count, 1, "backup should contain the seeded user");

        let _cleaned = std::fs::remove_file(&tmp);
    }

    #[test]
    fn scheduler_tick_creates_rolling_and_daily() {
        let tmp_dir = std::env::temp_dir().join("cp-backup-sched-test");
        let _created = std::fs::create_dir_all(&tmp_dir);
        let db_path = tmp_dir.join("auth.db");

        let store = AuthStore::open(&db_path).expect("open");
        let _user = store
            .create_user("sched@test.com", "Sched", "password1234", super::super::types::UserRole::User)
            .expect("create");

        let mut sched = BackupScheduler::new(db_path.clone());
        sched.tick(&store);

        // Rolling backup should have been created (first tick always fires).
        assert!(sched.rolling_path().exists(), "rolling backup should exist");

        // Daily backup should also have been created.
        assert!(
            !sched.last_daily_tag.is_empty(),
            "daily tag should be set after first tick"
        );
        let daily = sched.daily_path(&sched.last_daily_tag);
        assert!(daily.exists(), "daily backup should exist");

        // Clean up.
        let _removed = std::fs::remove_dir_all(&tmp_dir);
    }
}
