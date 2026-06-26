// ── Auth REST endpoints (SDK) ────────────────────────────────────────
//
// All 13 auth + ACL endpoints use the generated SDK.  Types are
// re-exported from generated/types.gen so existing imports keep working.

import type {
  AclEntry,
  AuthLogin,
  AuthStatus,
  AuthUser,
  CreateUserResponse,
  ForceLogoutResponse,
  OkResponse,
  RegisterResponse,
} from "./generated/types.gen"
import {
  deleteApiAgentByIdAclByUserId,
  deleteApiAuthUsersByUserId,
  getApiAgentByIdAcl,
  getApiAuthMe,
  getApiAuthStatus,
  getApiAuthUsers,
  patchApiAgentByIdAclByUserId,
  postApiAgentByIdAcl,
  postApiAuthLogin,
  postApiAuthLogout,
  postApiAuthRegister,
  postApiAuthUsers,
  postApiAuthUsersByUserIdLogout,
} from "./generated"
import { sdk } from "./client"

// ── Type re-exports (preserve import surface) ────────────────────────

export type { AuthUser, AclEntry, AuthStatus } from "./generated/types.gen"

// ── Auth status ──────────────────────────────────────────────────────

export function fetchAuthStatus(): Promise<AuthStatus> {
  return sdk(getApiAuthStatus())
}

// ── Auth actions ─────────────────────────────────────────────────────

export function authLogin(email: string, password: string): Promise<AuthLogin> {
  return sdk(postApiAuthLogin({ body: { email, password } }))
}

export function authRegister(
  email: string,
  name: string,
  password: string,
): Promise<RegisterResponse> {
  return sdk(postApiAuthRegister({ body: { email, name, password } }))
}

export function authLogout(): Promise<OkResponse> {
  return sdk(postApiAuthLogout())
}

export function authMe(): Promise<AuthUser> {
  return sdk(getApiAuthMe())
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
  /** Whether a Claude Code OAuth credential is present (keyless provider). */
  claude_oauth_connected: boolean
}

// ── Claude Code OAuth (manual paste flow) ────────────────────────────

/** Begin the Claude Code OAuth manual login → authorize URL to open. */
export function oauthStart(): Promise<{ authorize_url: string }> {
  return request("/api/auth/oauth/start", { method: "POST" })
}

/** Finish the OAuth login with the pasted `code#state`. Writes credentials. */
export function oauthFinish(code: string): Promise<{ ok: boolean }> {
  return request("/api/auth/oauth/finish", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code }),
  })
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

export function fetchUsers(): Promise<AuthUser[]> {
  return sdk(getApiAuthUsers())
}

export function createUser(
  email: string,
  name: string,
  password: string,
  role: "admin" | "user" = "user",
): Promise<CreateUserResponse> {
  return sdk(postApiAuthUsers({ body: { email, name, password, role } }))
}

export function deleteUser(userId: string): Promise<OkResponse> {
  return sdk(deleteApiAuthUsersByUserId({ path: { userId } }))
}

export function forceLogoutUser(userId: string): Promise<ForceLogoutResponse> {
  return sdk(postApiAuthUsersByUserIdLogout({ path: { userId } }))
}

// ── Per-agent ACL management ─────────────────────────────────────────

export function fetchAgentAcl(agentId: string): Promise<AclEntry[]> {
  return sdk(getApiAgentByIdAcl({ path: { id: agentId } }))
}

export function grantAccess(
  agentId: string,
  userId: string,
  role: "agent-admin" | "agent-user" = "agent-user",
): Promise<OkResponse> {
  return sdk(postApiAgentByIdAcl({
    path: { id: agentId },
    body: { user_id: userId, role },
  }))
}

export function updateAgentRole(
  agentId: string,
  userId: string,
  role: "agent-admin" | "agent-user",
): Promise<OkResponse> {
  return sdk(patchApiAgentByIdAclByUserId({
    path: { id: agentId, userId },
    body: { role },
  }))
}

export function revokeAccess(agentId: string, userId: string): Promise<OkResponse> {
  return sdk(deleteApiAgentByIdAclByUserId({ path: { id: agentId, userId } }))
}
