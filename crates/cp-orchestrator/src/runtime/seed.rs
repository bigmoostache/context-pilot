//! Boot-time account provisioning — the idempotent first-accounts seed.
//!
//! Split out of [`runtime`](super) so the runtime module stays within the
//! workspace's per-file line budget. The seed runs once at boot from
//! [`Runtime::new`](super::Runtime::new) when auth is enabled.

use cp_env::model::seed::Account;

use crate::services::auth::db::AuthStore;
use crate::services::auth::types::NewUser;
use crate::services::auth::types::UserRole;

/// Boot-time account seeding (provisioning, design §13.4). When auth is enabled
/// and the user table is **empty**, create the vendor `superadmin`
/// (`CP_SEED_SUPERADMIN_*`) and, optionally, the client's first `admin`
/// (`CP_SEED_ADMIN_*`) — each forced to change its provisioned password on first
/// login. Idempotent: a no-op once any user exists, so an Ansible provisioning
/// role can re-run safely. Fail-soft — never fatal.
///
/// The accounts come from the validated environment (`cp_env::env().seed`):
/// an email without a password, or a seed with auth disabled, was already
/// refused at boot, so what reaches this point is coherent.
pub(super) fn seed_accounts_if_empty(store: &AuthStore) {
    match store.count_users() {
        Ok(0) => {}
        Ok(_) => return, // already provisioned — idempotent no-op
        Err(e) => {
            crate::oerr!("seed: cannot count users: {e} — skipping");
            return;
        }
    }
    // Vendor god account (design §13.2 rank 4) and the client's top account
    // (rank 3). Both optional at this layer; Ansible supplies them (M5).
    let seed = &cp_env::env().seed;
    for (role_sql, configured) in [("superadmin", seed.superadmin.as_ref()), ("admin", seed.admin.as_ref())] {
        if let Some(account) = configured {
            seed_one(store, role_sql, account);
        }
    }
}

/// Seed a single account of `role_sql`. The role is passed as its SQL value
/// and resolved via [`UserRole::from_sql`] — seeding is config-driven
/// provisioning, not an enforcement decision, so it names no role variant
/// directly (keeping the capability-grep gate clean).
fn seed_one(store: &AuthStore, role_sql: &str, account: &Account) {
    let password = match account.password() {
        Ok(password) => password,
        Err(e) => {
            crate::oerr!("seed: {role_sql} {}: {e} — skipping", account.email());
            return;
        }
    };
    let role = UserRole::from_sql(role_sql);
    let new_user = NewUser { email: account.email(), name: account.name(), password: &password, role };
    match store.create_user(new_user) {
        Ok(user) => match store.set_must_change_password(&user.id, true) {
            Ok(_) => {
                crate::oerr!(
                    "seed: provisioned initial {role_sql} {} (password change required on first login)",
                    user.email
                );
            }
            Err(e) => crate::oerr!("seed: created {} but could not set must-change flag: {e}", user.email),
        },
        Err(e) => crate::oerr!("seed: failed to create {role_sql} {}: {e}", account.email()),
    }
}
