//! Boot-time admin provisioning — the idempotent first-Admin seed.
//!
//! Split out of [`runtime`](super) so the runtime module stays within the
//! workspace's per-file line budget. The seed runs once at boot from
//! [`Runtime::new`](super::Runtime::new) when auth is enabled.

use crate::services::auth::store::AuthStore;
use crate::services::auth::types::UserRole;

/// Boot-time admin seed (provisioning). When auth is enabled and the user table
/// is empty, create the first **Admin** from `CP_SEED_ADMIN_EMAIL` +
/// `CP_SEED_ADMIN_PASSWORD` (or `CP_SEED_ADMIN_PASSWORD_FILE`), forcing a
/// password change on first login. Idempotent: a no-op once any user exists, so
/// an Ansible provisioning role can re-run safely. Fail-soft — never fatal.
pub(super) fn seed_admin_if_empty(store: &AuthStore) {
    let Some(email) = std::env::var("CP_SEED_ADMIN_EMAIL").ok().filter(|s| !s.trim().is_empty()) else {
        return;
    };
    let Some(password) = seed_admin_password() else {
        eprintln!("seed-admin: CP_SEED_ADMIN_EMAIL set but no password provided — skipping");
        return;
    };
    match store.count_users() {
        Ok(0) => {}
        Ok(_) => return, // already provisioned — idempotent no-op
        Err(e) => {
            eprintln!("seed-admin: cannot count users: {e} — skipping");
            return;
        }
    }
    let name =
        std::env::var("CP_SEED_ADMIN_NAME").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "Admin".to_owned());
    match store.create_user(email.trim(), name.trim(), &password, UserRole::Admin) {
        Ok(user) => match store.set_must_change_password(&user.id, true) {
            Ok(_) => eprintln!(
                "seed-admin: provisioned initial admin {} (password change required on first login)",
                user.email
            ),
            Err(e) => eprintln!("seed-admin: created {} but could not set must-change flag: {e}", user.email),
        },
        Err(e) => eprintln!("seed-admin: failed to create admin {}: {e}", email.trim()),
    }
}

/// Resolve the seed admin password from `CP_SEED_ADMIN_PASSWORD_FILE` (preferred
/// — keeps the secret out of the process environment) or `CP_SEED_ADMIN_PASSWORD`.
fn seed_admin_password() -> Option<String> {
    if let Some(path) = std::env::var_os("CP_SEED_ADMIN_PASSWORD_FILE") {
        match std::fs::read_to_string(&path) {
            Ok(s) => {
                let pw = s.trim_end_matches(['\n', '\r']).to_owned();
                if !pw.is_empty() {
                    return Some(pw);
                }
            }
            Err(e) => eprintln!("seed-admin: cannot read CP_SEED_ADMIN_PASSWORD_FILE: {e}"),
        }
    }
    std::env::var("CP_SEED_ADMIN_PASSWORD").ok().filter(|s| !s.is_empty())
}
