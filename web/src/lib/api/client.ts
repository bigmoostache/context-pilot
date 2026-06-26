// ── Shared REST client primitives for the orchestration backend ──────
//
// Base URL from env (VITE_API_URL) with fallback to localhost:7878.
// Token helpers for Bearer auth.  `sdk()` casts the generated client's
// return type to match the runtime guarantee from setupClient.ts
// (throwOnError + responseStyle:'data').  `buildCommandEnvelope` wraps
// a command kind in the wire-protocol envelope.

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

/** SDK calls return `T` at runtime (throwOnError + responseStyle:'data'),
 *  but the generic defaults produce a wider type.  This cast is safe. */
export function sdk<T>(call: unknown): Promise<T> {
  return call as Promise<T>
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
