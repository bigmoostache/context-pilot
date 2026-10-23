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
  Branding,
  CreateUserResponse,
  Features,
  ForceLogoutResponse,
  OkResponse,
  RegisterResponse,
  SessionInfo,
} from "./generated/types.gen"
import {
  deleteApiFleetByIdAclByUserId,
  deleteApiAuthSessionsById,
  deleteApiAuthUsersByUserId,
  deleteApiSettingsFavicon,
  getApiBranding,
  getApiFleetByIdAcl,
  getApiAuthMe,
  getApiAuthSessions,
  getApiAuthStatus,
  getApiAuthUsers,
  getApiFeatures,
  getApiSettings,
  patchApiFleetByIdAclByUserId,
  patchApiAuthMe,
  postApiFleetByIdAcl,
  postApiAuthLogin,
  postApiAuthLogout,
  postApiAuthPassword,
  postApiAuthRegister,
  postApiAuthUsers,
  postApiAuthUsersByUserIdLogout,
  postApiSettings,
  postApiSettingsBranding,
} from "./generated"
import { sdk } from "./client"
import { sendUpload } from "./client/upload"

// ── Type re-exports (preserve import surface) ────────────────────────

export type {
  AuthUser,
  AuthMe,
  AclEntry,
  AuthStatus,
  AppSettings,
  Branding,
  Features,
} from "./generated/types.gen"

// ── Auth status ──────────────────────────────────────────────────────

export function fetchAuthStatus(): Promise<AuthStatus> {
  return sdk(getApiAuthStatus())
}

// ── Feature flags ────────────────────────────────────────────────────

/** The deployment's feature flags (`CP_FEATURE_*`; public, pre-login). */
export function fetchFeatures(): Promise<Features> {
  return sdk(getApiFeatures())
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

// ── Branding (tab title + favicon, orchestrator-wide) ────────────────

/** Tab title + favicon version; `null` fields mean the built-in default. Public. */
export function fetchBranding(): Promise<Branding> {
  return sdk(getApiBranding())
}

/** Admin: set the tab title (empty restores the default). */
export function updateBrandingTitle(title: string): Promise<Branding> {
  return sdk(postApiSettingsBranding({ body: { title } }))
}

/** Admin: replace the favicon with an uploaded image. */
export function uploadFavicon(file: File): Promise<Branding> {
  return sendUpload<Branding>("/api/settings/favicon", file, file.name || "favicon")
}

/** Admin: restore the built-in favicon. */
export function resetFavicon(): Promise<Branding> {
  return sdk(deleteApiSettingsFavicon())
}

/** URL of the uploaded favicon, cache-busted by its upload version. */
export function faviconUrl(version: string): string {
  const base = (import.meta.env["VITE_API_URL"] as string | undefined) || ""
  return `${base}/api/branding/favicon?v=${encodeURIComponent(version)}`
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

export function fetchFleetAcl(fleetId: string): Promise<AclEntry[]> {
  return sdk(getApiFleetByIdAcl({ path: { id: fleetId } }))
}

export function grantAccess(
  fleetId: string,
  userId: string,
  role: "agent-admin" | "agent-user" = "agent-user",
): Promise<OkResponse> {
  return sdk(
    postApiFleetByIdAcl({
      path: { id: fleetId },
      body: { user_id: userId, role },
    }),
  )
}

export function updateFleetRole(
  fleetId: string,
  userId: string,
  role: "agent-admin" | "agent-user",
): Promise<OkResponse> {
  return sdk(
    patchApiFleetByIdAclByUserId({
      path: { id: fleetId, userId },
      body: { role },
    }),
  )
}

export function revokeAccess(fleetId: string, userId: string): Promise<OkResponse> {
  return sdk(deleteApiFleetByIdAclByUserId({ path: { id: fleetId, userId } }))
}
