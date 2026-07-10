// ── System roles — the client-side mirror of the backend RBAC model ──
//
// The four ordered system roles (design §13.2): superadmin > admin > manager >
// user. `ROLE_RANK` mirrors the backend total order and drives which roles a
// user may assign, superadmin-row hiding, and user-management reach. Kept in a
// component-free module so it can be shared by both the auth dialog and the
// shell menu without tripping react-refresh's only-export-components rule.
// Server-side checks remain authoritative (NFR-05); this gating is cosmetic.

export type Role = "superadmin" | "admin" | "manager" | "user"

export const ROLE_ORDER: readonly Role[] = ["superadmin", "admin", "manager", "user"] as const
export const ROLE_RANK: Record<Role, number> = { superadmin: 4, admin: 3, manager: 2, user: 1 }

/** Roles the current user may assign: strictly below their own rank, except a
 *  superadmin who may assign any role (mirrors backend `can_assign_role`). */
export function assignableRoles(current: Role): Role[] {
  if (current === "superadmin") return [...ROLE_ORDER]
  return ROLE_ORDER.filter((r) => ROLE_RANK[r] < ROLE_RANK[current])
}

/** May the given role reach user management? Mirrors the backend capability
 *  `can_manage_users` (`role >= Manager`); server checks stay authoritative. */
export function canManageUsers(role: string | undefined): boolean {
  return (
    role != null && Object.hasOwn(ROLE_RANK, role) && ROLE_RANK[role as Role] >= ROLE_RANK.manager
  )
}
