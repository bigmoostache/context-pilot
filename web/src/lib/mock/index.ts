import type { User } from "../types"

// ── Surviving fixtures ────────────────────────────────────────────────
//
// The app is fully demaquetted (M63): every cockpit / usage / finder surface
// now renders live backend data, so the old maquette payloads (panels, threads,
// spine, usage, tree, radar, memory, entities, tool/queue/scratch/callback rows)
// were deleted. One fixture remains, still consumed by a live view that has no
// backend source yet:
//   • `currentUser` — the account identity shown when auth is disabled.

/** Local account identity — the fallback user when auth is off. */
export const currentUser: User = {
  name: "Guillaume Draznieks",
  email: "g.draznieks@gmail.com",
  initials: "GD",
  accent: "interactive",
  managedByCompany: false,
  company: undefined,
  role: "Admin",
}
