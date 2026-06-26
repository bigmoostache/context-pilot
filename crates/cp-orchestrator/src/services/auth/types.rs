//! Domain types for the auth subsystem — error, roles, user, session.

// ───────────────────────────── error type ─────────────────────────────

/// Errors returned by auth operations.
#[derive(Debug)]
pub(crate) enum AuthError {
    /// SQLite failure.
    Database(rusqlite::Error),
    /// Argon2 hashing / verification failure.
    Hash(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(err) => write!(f, "auth database error: {err}"),
            Self::Hash(msg) => write!(f, "password hash error: {msg}"),
        }
    }
}

impl From<rusqlite::Error> for AuthError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database(err)
    }
}

// ──────────────────────────── domain types ────────────────────────────

/// System-level role — controls orchestrator-wide permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum UserRole {
    /// God-mode: implicit access to all agents, can manage users.
    Admin,
    /// Regular authenticated user.
    User,
}

impl UserRole {
    /// SQL column value.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    /// Parse from the stored SQL text.  Falls back to [`User`](Self::User) on
    /// unknown values (forward-compat).
    pub(crate) fn from_sql(value: &str) -> Self {
        if value.eq_ignore_ascii_case("admin") { Self::Admin } else { Self::User }
    }
}

/// Per-agent role — stored in the `agent_acl` table (FR-14a).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AgentRole {
    /// Can invite/remove users and change roles on this agent (FR-14b).
    AgentAdmin,
    /// Can interact with the agent but cannot manage access.
    AgentUser,
}

impl AgentRole {
    /// SQL column value.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AgentAdmin => "agent-admin",
            Self::AgentUser => "agent-user",
        }
    }

    /// Parse from the stored SQL text.  Falls back to
    /// [`AgentUser`](Self::AgentUser) on unknown values.
    pub(crate) fn from_sql(value: &str) -> Self {
        if value.eq_ignore_ascii_case("agent-admin") { Self::AgentAdmin } else { Self::AgentUser }
    }
}

/// A registered user (row from `users`).
#[derive(Clone, Debug, serde::Serialize)]
pub struct User {
    /// UUID v4 (hex-formatted, 36 chars with dashes).
    pub(crate) id: String,
    /// Unique, case-insensitive email address.
    pub(crate) email: String,
    /// Display name.
    pub(crate) name: String,
    /// Argon2id PHC string — **never** serialised to the frontend.
    #[serde(skip)]
    pub(crate) password_hash: String,
    /// System-level role.
    pub(crate) role: UserRole,
    /// When `true`, the user must change their password before using the app —
    /// set on seeded/admin-provisioned accounts whose initial password is known
    /// to the provisioner. Cleared on the next successful password change.
    pub(crate) must_change_password: bool,
    /// Creation timestamp (ms since Unix epoch).
    pub(crate) created_at: u64,
    /// Last-update timestamp (ms since Unix epoch).
    pub(crate) updated_at: u64,
}

/// An access-control list entry — one user's permission on one agent,
/// joined with their profile info for display.
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct AclEntry {
    /// The agent this entry grants access to.
    pub(crate) agent_id: String,
    /// The authorized user's UUID.
    pub(crate) user_id: String,
    /// Per-agent role (FR-14a).
    pub(crate) role: AgentRole,
    /// When access was granted (ms since Unix epoch).
    pub(crate) granted_at: u64,
    /// UUID of the user who granted access (nullable).
    pub(crate) granted_by: Option<String>,
    /// Authorized user's email (joined from `users`).
    pub(crate) user_email: String,
    /// Authorized user's display name (joined from `users`).
    pub(crate) user_name: String,
}

/// One device session for the "active sessions" profile list — never exposes
/// the raw bearer token, only an opaque per-session `id` used for revocation.
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct SessionInfo {
    /// Opaque per-session id (safe to send to the client; not the token).
    pub(crate) id: String,
    /// Creation timestamp (ms since Unix epoch).
    pub(crate) created_at: u64,
    /// Absolute expiry (ms since Unix epoch).
    pub(crate) expires_at: u64,
    /// User-agent string captured at login, if any.
    pub(crate) user_agent: Option<String>,
    /// Whether this is the session making the request (the current device).
    pub(crate) current: bool,
}

/// Map a `rusqlite::Row` from the canonical `SELECT` column order into a
/// [`User`].  Column indices: 0=id, 1=email, 2=name, 3=password_hash,
/// 4=role, 5=created_at, 6=updated_at, 7=must_change_password.
pub(crate) fn row_to_user(row: &rusqlite::Row<'_>) -> Result<User, rusqlite::Error> {
    let role_str: String = row.get(4)?;
    Ok(User {
        id: row.get(0)?,
        email: row.get(1)?,
        name: row.get(2)?,
        password_hash: row.get(3)?,
        role: UserRole::from_sql(&role_str),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        must_change_password: row.get::<_, i64>(7)? != 0,
    })
}
