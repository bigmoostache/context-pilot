//! Core auth store — SQLite-backed user, session, and ACL persistence.
//!
//! [`AuthStore`] opens (and lazily creates) the auth database at an
//! orchestrator-level path, initialises the schema, and exposes password
//! hashing, token generation, and CRUD for users + sessions.

use std::path::Path;
use std::time::Duration;

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rusqlite::Connection;

use super::helpers::{fill_random, format_uuid, now_ms, random_hex};
use super::types::{AuthError, User, UserRole, row_to_user};

// ───────────────────────────── auth store ─────────────────────────────

/// Orchestrator-level authentication store backed by a dedicated SQLite
/// database (`~/.context-pilot/orchestrator/auth.db`).
///
/// Owns the database [`Connection`] directly — callers that need shared access
/// wrap it in an `Arc<Mutex<AuthStore>>`.
#[derive(Debug)]
pub struct AuthStore {
    /// The SQLite connection — WAL mode, foreign keys enforced.
    pub(crate) conn: Connection,
}

impl AuthStore {
    /// Open (or create) the auth database at `path` and initialise the schema.
    ///
    /// Parent directories are created if absent.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] if the file cannot be opened or the
    /// schema migration fails.
    pub(crate) fn open(path: &Path) -> Result<Self, AuthError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_io_err| {
                AuthError::Database(rusqlite::Error::InvalidPath(
                    parent.to_path_buf().into(),
                ))
            })?;
        }
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Create the three auth tables + indexes if they do not already exist.
    fn init_schema(&self) -> Result<(), AuthError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;

             CREATE TABLE IF NOT EXISTS users (
                 id           TEXT PRIMARY KEY,
                 email        TEXT NOT NULL UNIQUE COLLATE NOCASE,
                 name         TEXT NOT NULL,
                 password_hash TEXT NOT NULL,
                 role         TEXT NOT NULL DEFAULT 'user'
                                  CHECK(role IN ('admin', 'user')),
                 created_at   INTEGER NOT NULL,
                 updated_at   INTEGER NOT NULL
             );

             CREATE TABLE IF NOT EXISTS sessions (
                 token      TEXT PRIMARY KEY,
                 id         TEXT,
                 user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                 created_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL,
                 user_agent TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_sessions_user
                 ON sessions(user_id);
             CREATE INDEX IF NOT EXISTS idx_sessions_expires
                 ON sessions(expires_at);

             CREATE TABLE IF NOT EXISTS agent_acl (
                 agent_id   TEXT NOT NULL,
                 user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                 role       TEXT NOT NULL DEFAULT 'agent-user'
                                CHECK(role IN ('agent-admin', 'agent-user')),
                 granted_at INTEGER NOT NULL,
                 granted_by TEXT REFERENCES users(id) ON DELETE SET NULL,
                 PRIMARY KEY (agent_id, user_id)
             );
             CREATE INDEX IF NOT EXISTS idx_acl_user
                 ON agent_acl(user_id);",
        )?;
        // Migration for databases created before the per-session `id` column
        // existed — adds it if absent. The duplicate-column error on fresh
        // databases (where the CREATE already includes `id`) is expected and
        // ignored.
        let _migration = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN id TEXT", []);
        Ok(())
    }

    // ─────────────────── password hashing (NFR-01) ───────────────────

    /// Hash a plaintext password with Argon2id, returning the PHC-format
    /// string (contains algorithm, salt, params, and hash in one value).
    ///
    /// Uses [`OsRng`](argon2::password_hash::rand_core::OsRng) for salt
    /// generation — cryptographically secure, sourced from the OS entropy
    /// pool.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Hash`] if the Argon2 computation fails (should
    /// not happen under normal conditions).
    pub(crate) fn hash_password(plaintext: &str) -> Result<String, AuthError> {
        let mut salt_bytes = [0u8; 16];
        fill_random(&mut salt_bytes);
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|err| AuthError::Hash(err.to_string()))?;
        let argon2 = Argon2::default();
        let phc = argon2
            .hash_password(plaintext.as_bytes(), &salt)
            .map_err(|err| AuthError::Hash(err.to_string()))?;
        Ok(phc.to_string())
    }

    /// Verify a plaintext password against a stored PHC hash string.
    ///
    /// Returns `true` on match, `false` on mismatch.  The comparison is
    /// constant-time (Argon2 provides this natively, NFR-03).
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Hash`] only if the stored hash is malformed
    /// (not parseable as a PHC string).
    pub(crate) fn verify_password(stored_hash: &str, plaintext: &str) -> Result<bool, AuthError> {
        let parsed =
            PasswordHash::new(stored_hash).map_err(|err| AuthError::Hash(err.to_string()))?;
        Ok(Argon2::default()
            .verify_password(plaintext.as_bytes(), &parsed)
            .is_ok())
    }

    // ──────────────────── token / UUID generation ────────────────────

    /// Generate a 256-bit cryptographically random session token (64 hex
    /// chars), sourced from `/dev/urandom` (NFR-02).
    ///
    /// Mirrors the pattern in `transport::ticket` — no `rand` crate needed.
    pub(crate) fn generate_token() -> String {
        random_hex(32)
    }

    /// Generate a UUID v4 string (e.g. `550e8400-e29b-41d4-a716-446655440000`).
    ///
    /// 122 bits of entropy from `/dev/urandom`, version nibble = 4, variant
    /// bits = 10xx per RFC 4122 §4.4.
    pub(crate) fn generate_uuid() -> String {
        let mut bytes = [0u8; 16];
        fill_random(&mut bytes);
        // Set version 4 (bits 48..51 = 0100).
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        // Set variant 1 (bits 64..65 = 10).
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        format_uuid(&bytes)
    }

    // ─────────────────── user CRUD (Phase 2) ─────────────────────────

    /// Create a new user account, hashing the password with Argon2id.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on duplicate email (UNIQUE
    /// constraint) or any SQLite failure, [`AuthError::Hash`] if password
    /// hashing fails.
    pub(crate) fn create_user(
        &self,
        email: &str,
        name: &str,
        password: &str,
        role: UserRole,
    ) -> Result<User, AuthError> {
        let id = Self::generate_uuid();
        let hash = Self::hash_password(password)?;
        let now = now_ms();
        let _rows = self.conn.execute(
            "INSERT INTO users (id, email, name, password_hash, role, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, email, name, hash, role.as_str(), now, now],
        )?;
        Ok(User { id, email: email.to_owned(), name: name.to_owned(), password_hash: hash, role, created_at: now, updated_at: now })
    }

    /// Fetch a user by their UUID, or `None` if not found.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn get_user_by_id(&self, id: &str) -> Result<Option<User>, AuthError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, email, name, password_hash, role, created_at, updated_at \
             FROM users WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], row_to_user)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Fetch a user by their email address (case-insensitive), or `None`.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn get_user_by_email(&self, email: &str) -> Result<Option<User>, AuthError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, email, name, password_hash, role, created_at, updated_at \
             FROM users WHERE email = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![email], row_to_user)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// List all registered users, ordered by creation time.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn list_users(&self) -> Result<Vec<User>, AuthError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, email, name, password_hash, role, created_at, updated_at \
             FROM users ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], row_to_user)?;
        let mut users = Vec::new();
        for row in rows {
            users.push(row?);
        }
        Ok(users)
    }

    /// Delete a user by UUID.  Cascades to their sessions and ACL entries
    /// (FR-17).  Returns `true` if a row was actually deleted.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn delete_user(&self, id: &str) -> Result<bool, AuthError> {
        let deleted = self.conn.execute("DELETE FROM users WHERE id = ?1", rusqlite::params![id])?;
        Ok(deleted > 0)
    }

    /// Count registered users — used by the bootstrap check (FR-03: first
    /// register with zero users creates an admin).
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn count_users(&self) -> Result<u64, AuthError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    // ─────────────────── session CRUD (Phase 2) ──────────────────────

    /// Create a new session for `user_id`, returning the opaque token.
    ///
    /// The session expires after `ttl` from now.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure (e.g. foreign-key
    /// violation if `user_id` does not exist).
    pub(crate) fn create_session(
        &self,
        user_id: &str,
        user_agent: Option<&str>,
        ttl: Duration,
    ) -> Result<String, AuthError> {
        let token = Self::generate_token();
        let id = Self::generate_uuid();
        let now = now_ms();
        let ttl_ms = u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX);
        let expires_at = now.saturating_add(ttl_ms);
        let _rows = self.conn.execute(
            "INSERT INTO sessions (token, id, user_id, created_at, expires_at, user_agent) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![token, id, user_id, now, expires_at, user_agent],
        )?;
        Ok(token)
    }

    /// List a user's active (unexpired) sessions for the profile view, newest
    /// first. Never returns the raw token — only the opaque per-session `id`.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn list_sessions(
        &self,
        user_id: &str,
        current_token: Option<&str>,
    ) -> Result<Vec<super::types::SessionInfo>, AuthError> {
        let now = now_ms();
        let current = current_token.unwrap_or_default();
        let mut stmt = self.conn.prepare(
            "SELECT id, created_at, expires_at, user_agent, token \
             FROM sessions \
             WHERE user_id = ?1 AND expires_at > ?2 \
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![user_id, now], |row| {
            let token: String = row.get(4)?;
            Ok(super::types::SessionInfo {
                id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                created_at: row.get(1)?,
                expires_at: row.get(2)?,
                user_agent: row.get(3)?,
                current: !current.is_empty() && token == current,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Revoke a single session by its opaque `id`, scoped to `user_id` so a
    /// user can only drop their own devices. Returns `true` if a row was
    /// removed. Returns the revoked session's token so the caller can detect
    /// self-revocation.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn revoke_session_by_id(
        &self,
        user_id: &str,
        id: &str,
    ) -> Result<Option<String>, AuthError> {
        let token: Option<String> = self
            .conn
            .query_row(
                "SELECT token FROM sessions WHERE id = ?1 AND user_id = ?2",
                rusqlite::params![id, user_id],
                |row| row.get(0),
            )
            .ok();
        let deleted = self.conn.execute(
            "DELETE FROM sessions WHERE id = ?1 AND user_id = ?2",
            rusqlite::params![id, user_id],
        )?;
        Ok(if deleted > 0 { token } else { None })
    }

    /// Update a user's password hash (after the caller has verified the
    /// current password). Bumps `updated_at`. Returns `true` if a row changed.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Hash`] if hashing fails, [`AuthError::Database`] on
    /// SQLite failure.
    pub(crate) fn update_password(
        &self,
        user_id: &str,
        new_password: &str,
    ) -> Result<bool, AuthError> {
        let hash = Self::hash_password(new_password)?;
        let now = now_ms();
        let updated = self.conn.execute(
            "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![hash, now, user_id],
        )?;
        Ok(updated > 0)
    }

    /// Update a user's display name and email. Bumps `updated_at`. The unique
    /// email constraint surfaces as an [`AuthError::Database`] the caller maps
    /// to a 409.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure (incl. duplicate
    /// email).
    pub(crate) fn update_profile(
        &self,
        user_id: &str,
        name: &str,
        email: &str,
    ) -> Result<bool, AuthError> {
        let now = now_ms();
        let updated = self.conn.execute(
            "UPDATE users SET name = ?1, email = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![name, email, now, user_id],
        )?;
        Ok(updated > 0)
    }

    /// Validate a session token, returning the owning user if the token
    /// exists and has not expired.
    ///
    /// Lazily sweeps all expired sessions on every call (NFR-08).
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn validate_session(
        &self,
        token: &str,
    ) -> Result<Option<User>, AuthError> {
        let now = now_ms();
        // Lazy sweep — delete all expired sessions.
        let _swept = self.conn.execute(
            "DELETE FROM sessions WHERE expires_at <= ?1",
            rusqlite::params![now],
        )?;
        // Look up the session's user in one query (session columns not needed
        // externally — the Session struct was removed to eliminate dead_code on
        // fields only consumed by tests).
        let mut stmt = self.conn.prepare(
            "SELECT u.id, u.email, u.name, u.password_hash, u.role, u.created_at, u.updated_at \
             FROM sessions s \
             JOIN users u ON u.id = s.user_id \
             WHERE s.token = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![token], row_to_user)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Revoke (delete) a single session by its token.  Returns `true` if a
    /// session was actually removed.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on SQLite failure.
    pub(crate) fn revoke_session(&self, token: &str) -> Result<bool, AuthError> {
        let deleted = self.conn.execute(
            "DELETE FROM sessions WHERE token = ?1",
            rusqlite::params![token],
        )?;
        Ok(deleted > 0)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
