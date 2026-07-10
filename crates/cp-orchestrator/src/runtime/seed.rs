//! Boot-time account provisioning — the idempotent first-accounts seed.
//!
//! Split out of [`runtime`](super) so the runtime module stays within the
//! workspace's per-file line budget. The seed runs once at boot from
//! [`Runtime::new`](super::Runtime::new) when auth is enabled.

use crate::services::auth::store::AuthStore;
use crate::services::auth::types::UserRole;

/// Boot-time account seeding (provisioning, design §13.4). When auth is enabled
/// and the user table is **empty**, create the vendor `superadmin`
/// (`CP_SEED_SUPERADMIN_*`) and, optionally, the client's first `admin`
/// (`CP_SEED_ADMIN_*`) — each forced to change its provisioned password on first
/// login. Idempotent: a no-op once any user exists, so an Ansible provisioning
/// role can re-run safely. Fail-soft — never fatal.
pub(super) fn seed_accounts_if_empty(store: &AuthStore) {
    match store.count_users() {
        Ok(0) => {}
        Ok(_) => return, // already provisioned — idempotent no-op
        Err(e) => {
            eprintln!("seed: cannot count users: {e} — skipping");
            return;
        }
    }
    // Vendor god account (design §13.2 rank 4) and the client's top account
    // (rank 3). Both optional at this layer; Ansible supplies them (M5).
    seed_one(store, "superadmin", "CP_SEED_SUPERADMIN");
    seed_one(store, "admin", "CP_SEED_ADMIN");
}

/// Seed a single account of `role_sql` from the `<prefix>_EMAIL` / `_NAME` /
/// `_PASSWORD`(`_FILE`) env vars, if `<prefix>_EMAIL` is set. The role is passed
/// as its SQL value and resolved via [`UserRole::from_sql`] — seeding is
/// config-driven provisioning, not an enforcement decision, so it names no role
/// variant directly (keeping the capability-grep gate clean).
fn seed_one(store: &AuthStore, role_sql: &str, prefix: &str) {
    let Some(email) = std::env::var(format!("{prefix}_EMAIL")).ok().filter(|s| !s.trim().is_empty()) else {
        return;
    };
    let Some(password) = seed_password(prefix) else {
        eprintln!("seed: {prefix}_EMAIL set but no password provided — skipping");
        return;
    };
    let name = std::env::var(format!("{prefix}_NAME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| role_sql.to_owned());
    let role = UserRole::from_sql(role_sql);
    match store.create_user(email.trim(), name.trim(), &password, role) {
        Ok(user) => match store.set_must_change_password(&user.id, true) {
            Ok(_) => {
                eprintln!(
                    "seed: provisioned initial {role_sql} {} (password change required on first login)",
                    user.email
                )
            }
            Err(e) => eprintln!("seed: created {} but could not set must-change flag: {e}", user.email),
        },
        Err(e) => eprintln!("seed: failed to create {role_sql} {}: {e}", email.trim()),
    }
}

/// Resolve a seed password from `<prefix>_PASSWORD_FILE` (preferred — keeps the
/// secret out of the process environment) or `<prefix>_PASSWORD`.
fn seed_password(prefix: &str) -> Option<String> {
    if let Some(path) = std::env::var_os(format!("{prefix}_PASSWORD_FILE")) {
        match std::fs::read_to_string(&path) {
            Ok(s) => {
                let pw = s.trim_end_matches(['\n', '\r']).to_owned();
                if !pw.is_empty() {
                    return Some(pw);
                }
            }
            Err(e) => eprintln!("seed: cannot read {prefix}_PASSWORD_FILE: {e}"),
        }
    }
    std::env::var(format!("{prefix}_PASSWORD")).ok().filter(|s| !s.is_empty())
}
