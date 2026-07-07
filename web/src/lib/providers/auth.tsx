// ── Auth context — session lifecycle for the web frontend (Phase 9) ──
//
// This module holds the context object + the `useAuth` hook. The
// `AuthProvider` component (which drives the session lifecycle: status probe,
// token validation, login/register/logout) lives in `./AuthProvider` — split
// out so this module exports no component, satisfying the Fast-Refresh purity
// rule (hooks and components must not share a file).
//
// The companion AuthGuard component (components/auth/) renders the login page
// when auth is required but no valid session exists.

import { createContext, use } from "react"
import type { AuthMe } from "@/lib/api"

// ── Context shape ────────────────────────────────────────────────────

export interface AuthContextValue {
  /** The authenticated user (+ backend `next_action`), or null when logged
   *  out / auth disabled. */
  user: AuthMe | null
  /** Raw session token (mostly for debugging; Bearer injection is automatic). */
  token: string | null
  /** `true` = backend requires auth; `false` = auth disabled; `null` = still probing. */
  authEnabled: boolean | null
  /** True when ≥1 user exists (login mode); false = bootstrap-register mode. */
  bootstrapped: boolean
  /** True while the initial status + token validation is in flight. */
  loading: boolean
  /** Authenticate with email + password. Throws on failure. */
  login: (email: string, password: string) => Promise<void>
  /** Bootstrap-register the first user (admin). Throws on failure.
   *  After success, auto-logs in and sets `bootstrapped` to true. */
  register: (email: string, name: string, password: string) => Promise<void>
  /** End the current session and clear the stored token. */
  logout: () => Promise<void>
  /** Re-fetch `/me` and refresh the cached user (after a profile edit). */
  refreshMe: () => Promise<void>
}

/** Auth context object. Supplied by `AuthProvider`, read by {@link useAuth}. */
export const AuthContext = createContext<AuthContextValue | null>(null)

// ── Hook ─────────────────────────────────────────────────────────────

/** Read the auth context. Must be called inside an AuthProvider. */
export function useAuth(): AuthContextValue {
  const ctx = use(AuthContext)
  if (!ctx) throw new Error("useAuth must be used within AuthProvider")
  return ctx
}
