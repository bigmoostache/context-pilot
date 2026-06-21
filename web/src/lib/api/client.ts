// ── Shared REST client primitives for the orchestration backend ──────
//
// Base URL from env (VITE_API_URL) with fallback to localhost:7878.
// `request` returns typed JSON or throws on non-2xx with the response body.

export const BASE = import.meta.env.VITE_API_URL ?? "http://localhost:7878"

/** Typed fetch wrapper — throws on non-2xx with the response body. */
export async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, init)
  if (!res.ok) {
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
