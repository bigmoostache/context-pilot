//! Capability predicates — the enforcement primitives (design §13.3).
//!
//! Enforcement code MUST test one of these capabilities, never a role name
//! directly. This keeps the role matrix (§13.2) readable in one place and makes
//! adding a fifth role a one-line change to [`UserRole::rank`] instead of a
//! grep-and-pray across the transport handlers.
//!
//! Every predicate is a pure function of the caller's [`UserRole`], derived from
//! the total order in [`types`](super::types). When access control is disabled
//! the pipeline supplies no authenticated user at all (`auth_user == None`), so
//! the enforcement sites short-circuit to full access *before* ever reaching a
//! capability check — these predicates are only consulted once a real caller is
//! known (design §13.10).

use super::types::{User, UserRole};

impl User {
    /// Implicit access & management on **every** agent (design §13.3). Manager+.
    pub(crate) fn can_manage_all_agents(&self) -> bool {
        self.role >= UserRole::Manager
    }

    /// Create / list / delete / force-logout users of strictly lower rank
    /// (design §13.3, with anti-escalation via [`Self::can_assign_role`]). Manager+.
    pub(crate) fn can_manage_users(&self) -> bool {
        self.role >= UserRole::Manager
    }

    /// Manage IT infra — CA download + fingerprint, network identity, cert
    /// regeneration (design §13.3). Admin+.
    pub(crate) fn can_manage_it(&self) -> bool {
        self.role >= UserRole::Admin
    }

    /// Manage provider secrets — API keys + Claude OAuth (design §13.3).
    /// Superadmin only; this is the vendor-controlled billing boundary.
    pub(crate) fn can_manage_secrets(&self) -> bool {
        self.role == UserRole::Superadmin
    }

    /// Anti-escalation (FR-v3-03): may the caller create/promote/demote *to*
    /// `target`? Only to a role **strictly below** their own — nobody creates a
    /// peer or a superior, nobody escalates themselves — with the one carve-out
    /// that a `superadmin` may assign **any** role, including another
    /// `superadmin` (design §13.3 "superadmin→any"; FR-v3-05 "only a superadmin
    /// can create a superadmin"). This keeps vendor accounts creatable solely by
    /// the vendor while every client role stays strictly-below-only.
    pub(crate) fn can_assign_role(&self, target: UserRole) -> bool {
        self.role == UserRole::Superadmin || target < self.role
    }

    /// Vendor invisibility (FR-v3-05): may the caller see an account whose role
    /// is `other_role`? A `superadmin` account is visible only to another
    /// `superadmin`; every other role is visible to any `can_manage_users` holder.
    pub(crate) fn can_see(&self, other_role: UserRole) -> bool {
        other_role != UserRole::Superadmin || self.role == UserRole::Superadmin
    }
}

#[cfg(test)]
mod tests {
    use super::UserRole::{Admin, Manager, Superadmin, User as Regular};
    use super::*;

    /// Build a bare [`User`] with the given role — only `role` matters here.
    fn user(role: UserRole) -> User {
        User {
            id: "id".to_owned(),
            email: "e@x.com".to_owned(),
            name: "N".to_owned(),
            password_hash: String::new(),
            role,
            must_change_password: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// V0.2a — the exact truth table of design §13.3: 4 roles × 5 capabilities.
    #[test]
    fn capabilities_by_role() {
        // (role, all_agents, manage_users, manage_it, manage_secrets)
        let matrix = [
            (Superadmin, true, true, true, true),
            (Admin, true, true, true, false),
            (Manager, true, true, false, false),
            (Regular, false, false, false, false),
        ];
        for (role, all_agents, users, it, secrets) in matrix {
            let u = user(role);
            assert_eq!(u.can_manage_all_agents(), all_agents, "all_agents {role:?}");
            assert_eq!(u.can_manage_users(), users, "manage_users {role:?}");
            assert_eq!(u.can_manage_it(), it, "manage_it {role:?}");
            assert_eq!(u.can_manage_secrets(), secrets, "manage_secrets {role:?}");
        }
    }

    /// V0.2b — `can_assign_role` is true iff the target is strictly below self,
    /// except a `superadmin` who may assign any role (design §13.3 "superadmin→any").
    #[test]
    fn anti_escalation() {
        let all = [Superadmin, Admin, Manager, Regular];
        for &caller in &all {
            for &target in &all {
                let expected = target < caller || caller == Superadmin;
                assert_eq!(user(caller).can_assign_role(target), expected, "{caller:?} → {target:?}");
            }
        }
    }

    /// V0.2c — `can_see(Superadmin)` is true only for a superadmin caller; every
    /// other role is visible to everyone.
    #[test]
    fn vendor_invisibility() {
        for &caller in &[Superadmin, Admin, Manager, Regular] {
            assert_eq!(user(caller).can_see(Superadmin), caller == Superadmin, "see superadmin as {caller:?}");
            // Non-superadmin targets are always visible.
            for &target in &[Admin, Manager, Regular] {
                assert!(user(caller).can_see(target), "{caller:?} must see {target:?}");
            }
        }
    }
}
