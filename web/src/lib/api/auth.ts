// ── Auth REST endpoints (SDK) ────────────────────────────────────────
//
// All 13 auth + ACL endpoints use the generated SDK.  Types are
// re-exported from generated/types.gen so existing imports keep working.

import type {
  AclEntry,
  AppSettings,
  AuthLogin,
  AuthMe,
  AuthStatus,
  AuthUser,
  CreateUserResponse,
  ForceLogoutResponse,
  OkResponse,
  RegisterResponse,
  SessionInfo,
} from "./generated/types.gen"
import {
  deleteApiAgentByIdAclByUserId,
  deleteApiAuthSessionsById,
  deleteApiAuthUsersByUserId,
  getApiAgentByIdAcl,
  getApiAuthMe,
  getApiAuthSessions,
  getApiAuthStatus,
  getApiAuthUsers,
  getApiSettings,
  patchApiAgentByIdAclByUserId,
  patchApiAuthMe,
  postApiAgentByIdAcl,
  postApiAuthLogin,
  postApiAuthLogout,
  postApiAuthPassword,
  postApiAuthRegister,
  postApiAuthUsers,
  postApiAuthUsersByUserIdLogout,
  postApiSettings,
} from "./generated"
import { sdk } from "./client"

// ── Type re-exports (preserve import surface) ────────────────────────

export type { AuthUser, AuthMe, AclEntry, AuthStatus, AppSettings } from "./generated/types.gen"

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

/** Current user profile + the backend-driven post-login step (`next_action`). */
export function authMe(): Promise<AuthMe> {
  return sdk(getApiAuthMe())
}

// ── Self-serve profile (current user) ────────────────────────────────

/** Change the current user's password (verifies the current one). */
export function changePassword(current: string, next: string): Promise<OkResponse> {
  return sdk(postApiAuthPassword({ body: { current, new: next } }))
}

/** Update the current user's display name + email. Returns the refreshed user. */
export function updateProfile(name: string, email: string): Promise<{ user: AuthUser }> {
  return sdk(patchApiAuthMe({ body: { name, email } }))
}

/** One active device session (no raw token, just an opaque id). */
export type DeviceSession = SessionInfo

/** List the current user's active device sessions. */
export function fetchSessions(): Promise<DeviceSession[]> {
  return sdk<{ sessions: DeviceSession[] }>(getApiAuthSessions()).then((r) => r.sessions)
}

/** Revoke one of the current user's own sessions by id. */
export function revokeSession(id: string): Promise<OkResponse> {
  return sdk(deleteApiAuthSessionsById({ path: { id } }))
}

// ── Central settings (defaults, onboarding, provider keys) ───────────

/** Read central settings + onboarding state (any authenticated user). */
export function fetchSettings(): Promise<AppSettings> {
  return sdk(getApiSettings())
}

/** Admin: update new-agent defaults and/or the onboarding flag. */
export function updateSettings(patch: {
  default_provider?: string
  default_model?: string
  onboarding_completed?: boolean
  allowed_models?: string[]
  /** Access-control master flag (design §13.10). Asymmetric server gate:
   *  enable = anyone, disable = superadmin. */
  access_control?: boolean
}): Promise<AppSettings> {
  return sdk(postApiSettings({ body: patch }))
}

// ── Admin: user management ───────────────────────────────────────────

export function fetchUsers(): Promise<AuthUser[]> {
  return sdk(getApiAuthUsers())
}

export function createUser(
  email: string,
  name: string,
  password: string,
  role: "superadmin" | "admin" | "manager" | "user" = "user",
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
  return sdk(
    postApiAgentByIdAcl({
      path: { id: agentId },
      body: { user_id: userId, role },
    }),
  )
}

export function updateAgentRole(
  agentId: string,
  userId: string,
  role: "agent-admin" | "agent-user",
): Promise<OkResponse> {
  return sdk(
    patchApiAgentByIdAclByUserId({
      path: { id: agentId, userId },
      body: { role },
    }),
  )
}

export function revokeAccess(agentId: string, userId: string): Promise<OkResponse> {
  return sdk(deleteApiAgentByIdAclByUserId({ path: { id: agentId, userId } }))
}
