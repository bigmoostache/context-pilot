//! Per-agent access-control operations on [`AuthStore`].
//!
//! Extracted from `db.rs` to keep both files within the line budget.

use super::db::AuthStore;
use super::helpers::now_ms;
use super::types::{AccessGrant, AclEntry, AuthError, FleetRole};

impl AuthStore {
    /// Grant a user access to an agent with a specific per-agent role.
    ///
    /// Uses `INSERT OR REPLACE` — re-granting overwrites the previous entry
    /// (updating role, timestamp, granter).
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on foreign-key violation (unknown
    /// `user_id`) or other `SQLite` failure.
    pub(crate) fn grant_access(&self, grant: AccessGrant<'_>) -> Result<(), AuthError> {
        let AccessGrant { fleet_id, user_id, role, granted_by } = grant;
        let now = now_ms();
        let _rows = self.conn.execute(
            "INSERT OR REPLACE INTO agent_acl (agent_id, user_id, role, granted_at, granted_by) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![fleet_id, user_id, role.as_str(), now, granted_by],
        )?;
        Ok(())
    }

    /// Revoke a user's access to an agent.  Returns `true` if a row was
    /// actually deleted.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on `SQLite` failure.
    pub(crate) fn revoke_access(&self, fleet_id: &str, user_id: &str) -> Result<bool, AuthError> {
        let deleted = self.conn.execute(
            "DELETE FROM agent_acl WHERE agent_id = ?1 AND user_id = ?2",
            rusqlite::params![fleet_id, user_id],
        )?;
        Ok(deleted > 0)
    }

    /// Change a user's per-agent role.  Returns `true` if the row existed
    /// and was updated.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on `SQLite` failure.
    pub(crate) fn update_fleet_role(
        &self,
        fleet_id: &str,
        user_id: &str,
        new_role: FleetRole,
    ) -> Result<bool, AuthError> {
        let updated = self.conn.execute(
            "UPDATE agent_acl SET role = ?1 WHERE agent_id = ?2 AND user_id = ?3",
            rusqlite::params![new_role.as_str(), fleet_id, user_id],
        )?;
        Ok(updated > 0)
    }

    /// Check whether `user_id` has access to `fleet_id`, returning their
    /// per-agent role if so.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on `SQLite` failure.
    pub(crate) fn check_access(&self, fleet_id: &str, user_id: &str) -> Result<Option<FleetRole>, AuthError> {
        let mut stmt = self.conn.prepare("SELECT role FROM agent_acl WHERE agent_id = ?1 AND user_id = ?2")?;
        let mut rows = stmt.query_map(rusqlite::params![fleet_id, user_id], |row| {
            let role_str: String = row.get(0)?;
            Ok(FleetRole::from_sql(&role_str))
        })?;
        match rows.next() {
            Some(role) => Ok(Some(role?)),
            None => Ok(None),
        }
    }

    /// List all users with access to `fleet_id`, with their roles and
    /// profile info.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on `SQLite` failure.
    pub(crate) fn list_fleet_users(&self, fleet_id: &str) -> Result<Vec<AclEntry>, AuthError> {
        let mut stmt = self.conn.prepare(
            "SELECT a.agent_id, a.user_id, a.role, a.granted_at, a.granted_by, \
                    u.email, u.name \
             FROM agent_acl a \
             JOIN users u ON u.id = a.user_id \
             WHERE a.agent_id = ?1 \
             ORDER BY a.granted_at ASC, u.name ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![fleet_id], |row| {
            let role_str: String = row.get(2)?;
            Ok(AclEntry {
                fleet_id: row.get(0)?,
                user_id: row.get(1)?,
                role: FleetRole::from_sql(&role_str),
                granted_at: row.get(3)?,
                granted_by: row.get(4)?,
                user_email: row.get(5)?,
                user_name: row.get(6)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// List all agent IDs that `user_id` has access to.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on `SQLite` failure.
    pub(crate) fn list_user_fleets(&self, user_id: &str) -> Result<Vec<String>, AuthError> {
        let mut stmt =
            self.conn.prepare("SELECT agent_id FROM agent_acl WHERE user_id = ?1 ORDER BY granted_at ASC")?;
        let rows = stmt.query_map(rusqlite::params![user_id], |row| row.get(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    /// Convenience: is `user_id` an `agent-admin` on `fleet_id`?
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Database`] on `SQLite` failure.
    pub(crate) fn is_fleet_admin(&self, fleet_id: &str, user_id: &str) -> Result<bool, AuthError> {
        Ok(self.check_access(fleet_id, user_id)? == Some(FleetRole::FleetAdmin))
    }
}
