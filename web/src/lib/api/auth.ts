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
