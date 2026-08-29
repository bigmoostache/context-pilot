//! The SMS archive — a `SQLite` table beside the auth database.
//!
//! # Why the box keeps its own copy
//!
//! The modem is not storage. `mmcli --messaging-status` on the RM520N-GL
//! reports one storage (`mt`) holding a few dozen slots, and once it is full
//! **incoming messages are dropped in silence** — no error, no log, nothing the
//! cockpit could show. So the ingester's contract is: copy here first, then
//! delete from the modem. Photonicat's own stack reached the same conclusion
//! (`pcat-manager-web/app/pc_sms_client.py`: *"delete the message from
//! modem/SIM storage to prevent overflow"*).
//!
//! # Why [`Message::digest`] exists
//!
//! That contract has a failure mode: the copy succeeds and the delete does not
//! (modem busy, `ModemManager` restarting, box powered off in between). The next
//! poll then sees the same message again. Keying the table on the modem's own
//! index would not help — `ModemManager` reassigns indices freely across
//! re-enumerations, the same reason [`signal_dbm`] stopped hardcoding modem `0`.
//!
//! So identity is a **content digest**, and insertion is `INSERT OR IGNORE`.
//! A failed delete is then merely untidy: the next poll re-reads the message,
//! the insert is a no-op, and the delete is retried. Without it, every failed
//! delete would duplicate a message on every poll, forever.
//!
//! The file is `0600` and holds personal data — see [`prune`](SmsStore::prune)
//! for what bounds its lifetime.
//!
//! [`signal_dbm`]: super::super::status

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use sha2::{Digest as _, Sha256};

/// Which way a message travelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    /// Delivered to this box by the network (`pdu-type: deliver`).
    Received,
    /// Submitted by an operator through the cockpit (`pdu-type: submit`).
    Sent,
}

impl Direction {
    /// The column value. Stored as an integer because it is a closed two-valued
    /// fact, and because the read paths filter on it.
    const fn as_i64(self) -> i64 {
        match self {
            Self::Received => 0,
            Self::Sent => 1,
        }
    }

    /// The inverse of [`Self::as_i64`]; anything unexpected reads as
    /// `Received`, which is the direction that cannot be confused with an
    /// action this box took.
    const fn from_i64(raw: i64) -> Self {
        if raw == 1 { Self::Sent } else { Self::Received }
    }
}

/// Where a message is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// Ingested from the modem. Terminal for an inbound message.
    Received,
    /// Handed to the modem, no confirmation yet.
    Sending,
    /// The modem accepted it for delivery.
    Sent,
    /// The modem refused it, or the send never completed.
    Failed,
}

impl Delivery {
    /// The column value — a string, so the table stays readable to a human
    /// holding `sqlite3` during an incident.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }

    /// The inverse of [`Self::as_str`]. An unknown spelling reads as
    /// [`Failed`](Self::Failed): a row we cannot interpret must not be
    /// presented as delivered.
    fn from_str(raw: &str) -> Self {
        match raw {
            "received" => Self::Received,
            "sending" => Self::Sending,
            "sent" => Self::Sent,
            _unknown => Self::Failed,
        }
    }
}

/// One archived message, as the read paths hand it out.
#[derive(Clone, Debug)]
pub(crate) struct Message {
    /// Row id — the handle the cockpit uses to mark read or delete.
    pub id: i64,
    /// Which way it travelled.
    pub direction: Direction,
    /// The other end, in E.164 where the network gave us one. Senders may also
    /// be alphanumeric short names, which no dialling plan validates.
    pub peer: String,
    /// The text, already decoded to UTF-8 by `ModemManager`.
    pub body: String,
    /// The network's own timestamp, epoch seconds — `None` when the modem
    /// reported none (`mmcli` spells that `"--"`).
    pub sent_at: Option<i64>,
    /// When this box first saw it. Never null, so the list has a stable order
    /// even for messages the network did not timestamp.
    pub ingested_at: i64,
    /// Where it is in its life.
    pub delivery: Delivery,
    /// When an operator marked it read, or `None` while unread.
    pub read_at: Option<i64>,
    /// The user id that sent it — the audit trail for outbound messages, which
    /// cost money on the vendor's data plan. `None` for inbound.
    pub sent_by: Option<String>,
    /// Why a send failed, verbatim from the modem.
    pub error: Option<String>,
}

/// Epoch seconds, saturating rather than panicking on a pre-1970 clock.
pub(crate) fn now_s() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |dur| i64::try_from(dur.as_secs()).unwrap_or(i64::MAX))
}

/// The content digest that makes ingestion idempotent — see the module doc.
///
/// Deliberately independent of the modem's index and of `ingested_at`: the same
/// message re-read after a failed delete must hash the same, and it will not
/// have been re-timestamped by the network.
fn digest_of(direction: Direction, peer: &str, body: &str, sent_at: Option<i64>) -> String {
    let mut hasher = Sha256::new();
    // Fields go in as TEXT, not as raw integer bytes: the workspace forbids
    // both `to_le_bytes` and `to_be_bytes`, and a decimal rendering is
    // deterministic without having to pick a byte order at all.
    //
    // `peer` is length-prefixed rather than merely separated, so the boundary
    // between it and the body is unambiguous whatever either contains — a
    // separator alone would let a crafted body impersonate a different sender.
    let framed = format!("{}\u{1f}{}\u{1f}{}\u{1f}{peer}{body}", direction.as_i64(), sent_at.unwrap_or(0), peer.len());
    hasher.update(framed);
    format!("{:x}", hasher.finalize())
}

/// The archive's on-disk location: `CP_SMS_DB`, else beside `.network.json`.
pub(crate) fn sms_path(agents_dir: &Path) -> PathBuf {
    std::env::var_os("CP_SMS_DB").map_or_else(|| agents_dir.join("sms.db"), PathBuf::from)
}

/// The archive.
pub(crate) struct SmsStore {
    /// The connection. Single-writer by construction — the poller and the
    /// handlers share one [`SmsStore`] behind the backend mutex.
    conn: Connection,
}

impl SmsStore {
    /// Open (or create) the archive at `path` and install the schema.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the file cannot be opened or the
    /// schema cannot be applied.
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let conn = Connection::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let store = Self { conn };
        store.init_schema()?;
        restrict(path)?;
        Ok(store)
    }

    /// Create the table and its indexes if absent.
    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA busy_timeout = 5000;

                 CREATE TABLE IF NOT EXISTS sms (
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,
                     digest      TEXT    NOT NULL UNIQUE,
                     direction   INTEGER NOT NULL,
                     peer        TEXT    NOT NULL,
                     body        TEXT    NOT NULL,
                     sent_at     INTEGER,
                     ingested_at INTEGER NOT NULL,
                     delivery    TEXT    NOT NULL,
                     read_at     INTEGER,
                     sent_by     TEXT,
                     error       TEXT
                 );

                 CREATE INDEX IF NOT EXISTS sms_by_time ON sms(ingested_at DESC, id DESC);
                 CREATE INDEX IF NOT EXISTS sms_unread ON sms(read_at) WHERE read_at IS NULL;",
            )
            .map_err(|e| format!("sms schema: {e}"))
    }

    /// Archive one inbound message. `Ok(true)` when it was new.
    ///
    /// `INSERT OR IGNORE` on the digest is the whole idempotence story: the
    /// caller may re-offer the same message any number of times — after a
    /// failed modem delete, it will.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the insert cannot run at all.
    pub(crate) fn insert_incoming(&self, peer: &str, body: &str, sent_at: Option<i64>) -> Result<bool, String> {
        let digest = digest_of(Direction::Received, peer, body, sent_at);
        let changed = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO sms
                   (digest, direction, peer, body, sent_at, ingested_at, delivery)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    digest,
                    Direction::Received.as_i64(),
                    peer,
                    body,
                    sent_at,
                    now_s(),
                    Delivery::Received.as_str(),
                ],
            )
            .map_err(|e| format!("sms insert: {e}"))?;
        Ok(changed > 0)
    }

    /// Record an outbound message before it is handed to the modem, in
    /// [`Sending`](Delivery::Sending). Returns the row id.
    ///
    /// Written **first**, so a send that crashes mid-flight leaves a trace
    /// rather than vanishing: an operator can see what was attempted, and the
    /// `sent_by` audit survives.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the insert fails.
    pub(crate) fn record_outgoing(&self, peer: &str, body: &str, sent_by: Option<&str>) -> Result<i64, String> {
        let now = now_s();
        // The digest carries `now` for outbound: two identical messages sent
        // deliberately a minute apart are two messages, not one.
        let digest = digest_of(Direction::Sent, peer, body, Some(now));
        let _rows = self
            .conn
            .execute(
                "INSERT INTO sms
                   (digest, direction, peer, body, sent_at, ingested_at, delivery, read_at, sent_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?8)",
                rusqlite::params![
                    digest,
                    Direction::Sent.as_i64(),
                    peer,
                    body,
                    now,
                    now,
                    Delivery::Sending.as_str(),
                    sent_by,
                ],
            )
            .map_err(|e| format!("sms insert outgoing: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Move an outbound row to its terminal state.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the update fails.
    pub(crate) fn finish_outgoing(&self, id: i64, failure: Option<&str>) -> Result<(), String> {
        let delivery = if failure.is_some() { Delivery::Failed } else { Delivery::Sent };
        let _rows = self
            .conn
            .execute(
                "UPDATE sms SET delivery = ?1, error = ?2 WHERE id = ?3",
                rusqlite::params![delivery.as_str(), failure, id],
            )
            .map_err(|e| format!("sms finish: {e}"))?;
        Ok(())
    }

    /// One page of the archive, newest first. `before` is the id the previous
    /// page ended on, or `None` for the first page.
    ///
    /// Ordered by `ingested_at` then `id`, never by `sent_at`: the network's
    /// timestamp is optional and, on a message that arrived while the modem's
    /// clock was unset, wrong. Ingestion order is the one this box can vouch for.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the query fails.
    pub(crate) fn list(&self, before: Option<i64>, limit: i64) -> Result<Vec<Message>, String> {
        let sql = format!(
            "SELECT {SELECT_COLUMNS} FROM sms
             WHERE (?1 IS NULL OR id < ?1)
             ORDER BY ingested_at DESC, id DESC
             LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| format!("sms list: {e}"))?;
        let rows =
            stmt.query_map(rusqlite::params![before, limit], row_to_message).map_err(|e| format!("sms list: {e}"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| format!("sms list: {e}"))
    }

    /// How many inbound messages nobody has opened yet — the cockpit's badge.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the query fails.
    pub(crate) fn unread_count(&self) -> Result<i64, String> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM sms WHERE read_at IS NULL AND direction = ?1",
                rusqlite::params![Direction::Received.as_i64()],
                |row| row.get(0),
            )
            .map_err(|e| format!("sms unread: {e}"))
    }

    /// Mark one message read. `Ok(false)` when no such row exists.
    ///
    /// Idempotent: re-marking keeps the first timestamp, because when it was
    /// *first* seen is the fact worth keeping.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the update fails.
    pub(crate) fn mark_read(&self, id: i64) -> Result<bool, String> {
        let changed = self
            .conn
            .execute("UPDATE sms SET read_at = ?1 WHERE id = ?2 AND read_at IS NULL", rusqlite::params![now_s(), id])
            .map_err(|e| format!("sms mark read: {e}"))?;
        if changed > 0 {
            return Ok(true);
        }
        // Already read is still "this message exists" — distinguish it from a
        // bad id so the caller can answer 404 only when it means it.
        self.exists(id)
    }

    /// Whether a row with this id is present.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the query fails.
    fn exists(&self, id: i64) -> Result<bool, String> {
        use rusqlite::OptionalExtension as _;
        self.conn
            .query_row("SELECT 1 FROM sms WHERE id = ?1", rusqlite::params![id], |_row| Ok(()))
            .optional()
            .map(|found| found.is_some())
            .map_err(|e| format!("sms exists: {e}"))
    }

    /// Delete one message from the archive. `Ok(false)` when no such row.
    ///
    /// This deletes only our copy — the modem's has already been removed by the
    /// ingester, which is what keeps its storage from filling.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the delete fails.
    pub(crate) fn delete(&self, id: i64) -> Result<bool, String> {
        let changed = self
            .conn
            .execute("DELETE FROM sms WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("sms delete: {e}"))?;
        Ok(changed > 0)
    }

    /// How many messages `sender` has sent since `since` — the rate limiter's
    /// only question. `None` counts every sender, for the global ceiling.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when the query fails.
    pub(crate) fn sent_since(&self, since: i64, sender: Option<&str>) -> Result<i64, String> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM sms
                 WHERE direction = ?1 AND ingested_at >= ?2
                   AND (?3 IS NULL OR sent_by = ?3)",
                rusqlite::params![Direction::Sent.as_i64(), since, sender],
                |row| row.get(0),
            )
            .map_err(|e| format!("sms sent_since: {e}"))
    }

    /// Enforce the retention policy: drop anything older than `max_age_days`,
    /// then anything beyond the newest `max_rows`. Returns how many rows went.
    ///
    /// Two bounds, not one, because they fail differently. Age alone lets a
    /// flood fill the disk inside the window; volume alone keeps a quiet box's
    /// messages forever. These are personal data on a client's appliance, so
    /// both bounds ship in v1 rather than being added after the first audit.
    ///
    /// `now` is passed in rather than read here, so the age bound is testable
    /// without a test-only backdoor to backdate rows — the clock is the one
    /// input that would otherwise make this function untestable in-process.
    ///
    /// # Errors
    ///
    /// Returns the `SQLite` message when either delete fails.
    pub(crate) fn prune(&self, now: i64, max_age_days: i64, max_rows: i64) -> Result<usize, String> {
        const SECONDS_PER_DAY: i64 = 86_400;
        let cutoff = now.saturating_sub(max_age_days.saturating_mul(SECONDS_PER_DAY));
        let by_age = self
            .conn
            .execute("DELETE FROM sms WHERE ingested_at < ?1", rusqlite::params![cutoff])
            .map_err(|e| format!("sms prune by age: {e}"))?;
        let by_volume = self
            .conn
            .execute(
                "DELETE FROM sms WHERE id NOT IN (
                     SELECT id FROM sms ORDER BY ingested_at DESC, id DESC LIMIT ?1
                 )",
                rusqlite::params![max_rows],
            )
            .map_err(|e| format!("sms prune by volume: {e}"))?;
        Ok(by_age.saturating_add(by_volume))
    }
}

/// `chmod 0600` — the archive holds personal data, exactly like `.network.json`
/// holds the PSK and the SIM PIN.
#[cfg(unix)]
fn restrict(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod {}: {e}", path.display()))
}

/// No-op on non-Unix (local dev on a platform without POSIX modes).
#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Build a [`Message`] from a row selected with [`SELECT_COLUMNS`].
fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get("id")?,
        direction: Direction::from_i64(row.get("direction")?),
        peer: row.get("peer")?,
        body: row.get("body")?,
        sent_at: row.get("sent_at")?,
        ingested_at: row.get("ingested_at")?,
        delivery: Delivery::from_str(&row.get::<_, String>("delivery")?),
        read_at: row.get("read_at")?,
        sent_by: row.get("sent_by")?,
        error: row.get("error")?,
    })
}

/// The column list every read path selects, so [`row_to_message`] can name its
/// columns rather than count them.
const SELECT_COLUMNS: &str = "id, direction, peer, body, sent_at, ingested_at, delivery, read_at, sent_by, error";
