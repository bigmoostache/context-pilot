// ── Auth REST endpoints (Phase 9) ────────────────────────────────────
//
// Split from index.ts to keep files under the 500-line budget.
// Re-exported from index.ts so `@/lib/api` remains the single import surface.

import { request } from "./client"

// ── Auth status ──────────────────────────────────────────────────────

/** Backend auth status — always accessible, no Bearer needed.
 *  `bootstrapped` is true when at least one user exists (login mode);
 *  false means the first register will create the admin (bootstrap mode). */
export function fetchAuthStatus(): Promise<{ enabled: boolean; bootstrapped: boolean }> {
  return request("/api/auth/status")
}

// ── Auth types ───────────────────────────────────────────────────────

/** Auth user shape returned by login/me/register. */
export interface AuthUser {
  id: string
  email: string
  name: string
  role: "admin" | "user"
  created_at: number
}

// ── Auth actions ─────────────────────────────────────────────────────

/** Login with email + password → session token + user profile. */
export function authLogin(
  email: string,
  password: string,
): Promise<{ token: string; user: AuthUser }> {
  return request("/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password }),
  })
}

/** Register a new user. Bootstrap (zero users) creates admin; otherwise
 *  requires admin session (Bearer auto-injected by client). */
export function authRegister(
  email: string,
  name: string,
  password: string,
): Promise<{ user: AuthUser }> {
  return request("/api/auth/register", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, name, password }),
  })
}

/** Destroy the current session. */
export function authLogout(): Promise<{ ok: boolean }> {
  return request("/api/auth/logout", { method: "POST" })
}

/** Current user profile (validates the stored token). */
export function authMe(): Promise<AuthUser> {
  return request<AuthUser>("/api/auth/me")
}

// ── Self-serve profile (current user) ────────────────────────────────

/** Change the current user's password (verifies the current one). */
export function changePassword(
  current: string,
  next: string,
): Promise<{ ok: boolean }> {
  return request("/api/auth/password", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ current, new: next }),
  })
}

/** Update the current user's display name + email. Returns the refreshed user. */
export function updateProfile(
  name: string,
  email: string,
): Promise<{ user: AuthUser }> {
  return request("/api/auth/me", {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, email }),
  })
}

/** One active device session (no raw token, just an opaque id). */
export interface DeviceSession {
  id: string
  created_at: number
  expires_at: number
  user_agent: string | null
  current: boolean
}

/** List the current user's active device sessions. */
export function fetchSessions(): Promise<DeviceSession[]> {
  return request<{ sessions: DeviceSession[] }>("/api/auth/sessions").then(
    (r) => r.sessions,
  )
}

/** Revoke one of the current user's own sessions by id. */
export function revokeSession(id: string): Promise<{ ok: boolean }> {
  return request(`/api/auth/sessions/${id}`, { method: "DELETE" })
}

// ── Central settings (defaults, onboarding, provider keys) ───────────

/** One provider's key-configured state (never the key value). */
export interface ProviderKeyState {
  id: string
  configured: boolean
}

/** Central cockpit settings (server-side defaults + onboarding gate). */
export interface AppSettings {
  default_provider: string | null
  default_model: string | null
  onboarding_completed: boolean
  is_admin: boolean
  auth_enabled: boolean
  providers: ProviderKeyState[]
}

/** Read central settings + onboarding state (any authenticated user). */
export function fetchSettings(): Promise<AppSettings> {
  return request("/api/settings")
}

/** Admin: update new-agent defaults and/or the onboarding flag. */
export function updateSettings(patch: {
  default_provider?: string
  default_model?: string
  onboarding_completed?: boolean
}): Promise<AppSettings> {
  return request("/api/settings", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(patch),
  })
}

/** Admin: store provider API keys centrally. Empty value clears a key. */
export function updateProviderKeys(
  keys: Record<string, string>,
): Promise<AppSettings> {
  return request("/api/settings/keys", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ keys }),
  })
}

// ── Admin: user management ───────────────────────────────────────────

/** List all registered users (admin only). */
export function fetchUsers(): Promise<AuthUser[]> {
  return request("/api/auth/users")
}

/** Admin: create a new user account. */
export function createUser(
  email: string,
  name: string,
  password: string,
  role: "admin" | "user" = "user",
): Promise<{ user: AuthUser }> {
  return request("/api/auth/users", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, name, password, role }),
  })
}

/** Admin: delete a user (cascades sessions + ACL). */
export function deleteUser(userId: string): Promise<{ ok: boolean }> {
  return request(`/api/auth/users/${userId}`, { method: "DELETE" })
}

/** Admin: force-logout a user (revoke all their sessions). */
export function forceLogoutUser(
  userId: string,
): Promise<{ ok: boolean; revoked_sessions: number }> {
  return request(`/api/auth/users/${userId}/logout`, { method: "POST" })
}

// ── Per-agent ACL management ─────────────────────────────────────────

/** ACL entry returned by the agent ACL endpoints. */
export interface AclEntry {
  agent_id: string
  user_id: string
  role: "agent-admin" | "agent-user"
  granted_at: number
  granted_by: string | null
  user_email: string
  user_name: string
}

/** List users with access to an agent (admin or agent-admin). */
export function fetchAgentAcl(agentId: string): Promise<AclEntry[]> {
  return request(`/api/agent/${agentId}/acl`)
}

/** Grant a user access to an agent. */
export function grantAccess(
  agentId: string,
  userId: string,
  role: "agent-admin" | "agent-user" = "agent-user",
): Promise<{ ok: boolean }> {
  return request(`/api/agent/${agentId}/acl`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ user_id: userId, role }),
  })
}

/** Change a user's per-agent role. */
export function updateAgentRole(
  agentId: string,
  userId: string,
  role: "agent-admin" | "agent-user",
): Promise<{ ok: boolean }> {
  return request(`/api/agent/${agentId}/acl/${userId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ role }),
  })
}

/** Revoke a user's access to an agent. */
export function revokeAccess(
  agentId: string,
  userId: string,
): Promise<{ ok: boolean }> {
  return request(`/api/agent/${agentId}/acl/${userId}`, { method: "DELETE" })
}
