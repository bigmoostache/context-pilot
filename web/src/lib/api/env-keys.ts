// ── Env-key inspection endpoints (T399) ──────────────────────────────
//
// On-demand API key status + masked reveal. Keys are never loaded into
// the frontend until the user explicitly clicks the reveal (eye) button.
// When auth is active, only admins may reveal masked values.

import { request } from "./client"

/** Status of a single well-known env var on the orchestrator. */
export interface EnvKeyStatus {
  env: string
  label: string
  exists: boolean
}

/** Masked reveal of a single env var's value. */
export interface EnvKeyReveal {
  env: string
  value: string | null
  masked: string | null
  exists: boolean
}

/** Fetch the exists/missing status of all well-known env vars.
 *  No key values are included — callers learn *whether* a key is set. */
export function fetchEnvKeys(): Promise<EnvKeyStatus[]> {
  return request("/api/env-keys")
}

/** Reveal a masked env-key value (admin-only when auth is active).
 *  The raw value never leaves the orchestrator — only first-4 + last-4
 *  characters with the middle redacted. */
export function revealEnvKey(name: string): Promise<EnvKeyReveal> {
  return request(`/api/env-keys/${encodeURIComponent(name)}`)
}

/** Update an env-key value (admin-only).  Persists to ~/.context-pilot/.env
 *  and stores an in-memory override for immediate visibility. */
export function updateEnvKey(
  name: string,
  value: string,
): Promise<EnvKeyReveal> {
  return request(`/api/env-keys/${encodeURIComponent(name)}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ value }),
  })
}
