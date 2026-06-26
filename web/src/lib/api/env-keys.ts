// ── Env-key inspection + editing endpoints (SDK) ────────────────────
//
// On-demand API key status + masked reveal.  Types are re-exported from
// generated/types.gen so existing imports keep working.

import type {
  EnvKeyReveal,
  EnvKeyStatus,
  EnvKeyUpdateResult,
} from "./generated/types.gen"
import {
  getApiEnvKeys,
  getApiEnvKeysByName,
  putApiEnvKeysByName,
} from "./generated"
import { sdk } from "./client"

// ── Type re-exports (preserve import surface) ────────────────────────

export type { EnvKeyStatus, EnvKeyReveal, EnvKeyUpdateResult } from "./generated/types.gen"

// ── Endpoints ────────────────────────────────────────────────────────

/** Fetch exists/missing status of all well-known env vars. */
export function fetchEnvKeys(): Promise<EnvKeyStatus[]> {
  return sdk(getApiEnvKeys())
}

/** Reveal a masked env-key value (admin-only when auth is active). */
export function revealEnvKey(name: string): Promise<EnvKeyReveal> {
  return sdk(getApiEnvKeysByName({ path: { name } }))
}

/** Update an env-key value (admin-only).  Persists to ~/.context-pilot/.env. */
export function updateEnvKey(name: string, value: string): Promise<EnvKeyUpdateResult> {
  return sdk(putApiEnvKeysByName({ path: { name }, body: { value } }))
}
