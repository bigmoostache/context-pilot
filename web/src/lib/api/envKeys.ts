// ── Env-key inspection + editing endpoints (SDK) ────────────────────
//
// On-demand API key status + masked reveal.  Types are re-exported from
// generated/types.gen so existing imports keep working.

import type { EnvKeyStatus, EnvKeyReveal, EnvKeyUpdateResult } from "./generated/types.gen"
import { getApiEnvKeys, getApiEnvKeysByName, putApiEnvKeysByName } from "./generated"
import { sdk } from "./client"

// ── Type re-exports (preserve import surface) ────────────────────────

export type { EnvKeyStatus, EnvKeyReveal, EnvKeyUpdateResult } from "./generated/types.gen"

// ── Endpoints ────────────────────────────────────────────────────────

/** Fetch exists/missing status of all well-known env vars. */
export function fetchEnvKeys(): Promise<EnvKeyStatus[]> {
  return sdk(getApiEnvKeys())
}

/** Reveal a single env-key's current value (superadmin — `can_manage_secrets`).
 *  `name` accepts either the env var (`ANTHROPIC_API_KEY`) or canonical
 *  (`anthropic`) form. The server enforces the capability (403 otherwise). */
export function revealEnvKey(name: string): Promise<EnvKeyReveal> {
  return sdk(getApiEnvKeysByName({ path: { name } }))
}

/** Update (persist) a single env-key's value (superadmin — `can_manage_secrets`).
 *  Writes to `~/.context-pilot/.env` and sets an in-memory override. */
export function updateEnvKey(name: string, value: string): Promise<EnvKeyUpdateResult> {
  return sdk(putApiEnvKeysByName({ path: { name }, body: { value } }))
}
