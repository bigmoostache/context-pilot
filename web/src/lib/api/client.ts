// ── Shared REST client primitives for the orchestration backend ──────
//
// Base URL from env (VITE_API_URL) with fallback to localhost:7878.
// `request` returns typed JSON or throws on non-2xx with the response body.
// When auth is enabled, every request automatically carries the Bearer token
// from localStorage; a 401 on a previously-authenticated call clears the
// token and signals the AuthProvider to show the login page.

export const BASE = import.meta.env.VITE_API_URL ?? "http://localhost:7878"

/** localStorage key for the auth session token. */
const TOKEN_KEY = "cp-auth-token"

/** Read the persisted Bearer token (or null). */
export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY)
}

/** Persist or clear the Bearer token. */
export function setToken(token: string | null) {
  if (token) localStorage.setItem(TOKEN_KEY, token)
  else localStorage.removeItem(TOKEN_KEY)
}

/** Typed fetch wrapper — auto-injects Bearer token, throws on non-2xx. */
export async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const token = getToken()
  const headers = new Headers(init?.headers)
  if (token && !headers.has("Authorization")) {
    headers.set("Authorization", `Bearer ${token}`)
  }

  const res = await fetch(`${BASE}${path}`, { ...init, headers })
  if (!res.ok) {
    // Session expired or revoked — clear token and notify AuthProvider.
    // Only fire when we HAD a token (avoids triggering on login 401s).
    if (res.status === 401 && token) {
      setToken(null)
      window.dispatchEvent(new Event("cp-auth-expired"))
    }
    const body = await res.text().catch(() => res.statusText)
    throw new Error(`${res.status} ${path}: ${body}`)
  }
  return res.json() as Promise<T>
}

/** Build a full Command envelope around a Kind payload. */
export function buildCommandEnvelope(kind: Record<string, unknown>): object {
  return {
    schema_version: 1,
    id: crypto.randomUUID(),
    seq: 0,
    dedup_token: crypto.randomUUID(),
    kind,
  }
}
