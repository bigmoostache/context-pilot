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
