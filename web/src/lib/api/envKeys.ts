// ── Env-key inspection + editing endpoints (SDK) ────────────────────
//
// On-demand API key status + masked reveal.  Types are re-exported from
// generated/types.gen so existing imports keep working.

import type { EnvKeyStatus } from "./generated/types.gen"
import { getApiEnvKeys } from "./generated"
import { sdk } from "./client"

// ── Type re-exports (preserve import surface) ────────────────────────

export type { EnvKeyStatus, EnvKeyReveal, EnvKeyUpdateResult } from "./generated/types.gen"

// ── Endpoints ────────────────────────────────────────────────────────

/** Fetch exists/missing status of all well-known env vars. */
export function fetchEnvKeys(): Promise<EnvKeyStatus[]> {
  return sdk(getApiEnvKeys())
}
