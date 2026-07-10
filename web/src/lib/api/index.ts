// ── REST API client for the orchestration backend ────────────────────
//
// Every endpoint uses the generated SDK from openapi.json — zero manual
// fetch calls.  Thin presentation wrappers (formatAge) reshape backend
// data for UI consumption; the API contract itself is
// enforced by the generated types.
//
// The sub-modules (auth, finder, body, envKeys) are re-exported so
// `@/lib/api` remains the single import surface.
//
// NOTE: setupClient.ts configures the hey-api singleton with
// `throwOnError: true` + `responseStyle: 'data'`, so SDK calls return
// data directly and throw on non-2xx.  TypeScript generics default to
// `ThrowOnError = false`, hence the `as` casts below — they align the
// compile-time type with the runtime guarantee.

import type { Agent, ThreadDetail } from "../types"
import type {
  AgentMetrics,
  CommandReceipt as GenCommandReceipt,
  CreateAgentReceipt,
  CreateCommandReceipt,
  LibraryItem,
  RestartReceipt,
  RetireReceipt,
  UnretireReceipt,
  Vital,
  ClaudeUsageResponse,
} from "./generated/types.gen"
import {
  getApiClaudeUsage,
  getApiClaudeLoginStatus,
  postApiClaudeLoginStart,
  postApiClaudeLoginComplete,
  postApiClaudeLoginRefresh,
  getApiClaudeAccounts,
  postApiClaudeAccountsStore,
  postApiClaudeAccountsSwitch,
  deleteApiClaudeAccountsByEmail,
  getApiFleetMeta,
  getApiFleetRetired,
  getApiAgentByIdMeta,
  getApiAgentByIdMetrics,
  getApiAgentByIdVitals,
  getApiAgentByIdLibrary,
  getApiAgentByIdThreads,
  getApiProviders,
  getApiUpdateStatus,
  postApiUpdateCheck,
  postApiUpdateApply,
  putApiUpdateMode,
  getApiMetrics,
  postApiFleetCreate,
  postApiAgentByIdRestart,
  postApiAgentByIdRetire,
  postApiAgentByIdUnretire,
  postApiAgentByIdRename,
  postApiAgentByIdAvatar,
  postApiAgentByIdCommand,
  postApiAgentByIdLibraryCommand,
  postApiTicket,
} from "./generated"
import { buildCommandEnvelope, sdk } from "./client"

export { getToken, setToken } from "./client"
export * from "./auth"
export * from "./finder"
export * from "./body"
export * from "./envKeys"
export * from "./it"

// ── Type re-exports ───────────────────────────────────────────────────

export type { CreateAgentReceipt } from "./generated/types.gen"
export type { RestartReceipt } from "./generated/types.gen"
export type { RetireReceipt, UnretireReceipt } from "./generated/types.gen"
export type { AgentMetrics } from "./generated/types.gen"
export type { Vital } from "./generated/types.gen"
export type { CreateCommandReceipt } from "./generated/types.gen"
export type { ClaudeUsageResponse, ClaudeUsageLimit } from "./generated/types.gen"
export type {
  ClaudeTokenStatus,
  ClaudeLoginStartResponse,
  ClaudeLoginCompleteResponse,
} from "./generated/types.gen"
export type { ClaudeAccountSummary, ClaudeAccountsListResponse } from "./generated/types.gen"

// ── Helper: align TS with runtime (setupClient.ts guarantees) ─────────

// ── Fleet (SDK) ───────────────────────────────────────────────────────

export function fetchFleet(): Promise<Agent[]> {
  return sdk(getApiFleetMeta())
}

export function fetchRetiredFleet(): Promise<Agent[]> {
  return sdk(getApiFleetRetired())
}

// ── Agent lifecycle (SDK) ─────────────────────────────────────────────

export function createAgent(body: {
  name: string
  folder?: string
  model?: string
}): Promise<CreateAgentReceipt> {
  return sdk(postApiFleetCreate({ body }))
}

export function restartAgent(agentId: string): Promise<RestartReceipt> {
  return sdk(postApiAgentByIdRestart({ path: { id: agentId } }))
}

export function retireAgent(agentId: string): Promise<RetireReceipt> {
  return sdk(postApiAgentByIdRetire({ path: { id: agentId } }))
}

export function unretireAgent(agentId: string): Promise<UnretireReceipt> {
  return sdk(postApiAgentByIdUnretire({ path: { id: agentId } }))
}

export function renameAgent(agentId: string, name: string): Promise<{ ok: boolean }> {
  return sdk(postApiAgentByIdRename({ path: { id: agentId }, body: { name } }))
}

// ── Agent meta (SDK) ──────────────────────────────────────────────────

export function fetchAgentMeta(agentId: string): Promise<Agent> {
  return sdk(getApiAgentByIdMeta({ path: { id: agentId } }))
}

// ── Agent avatar ──────────────────────────────────────────────────────

export function uploadAvatar(agentId: string, file: File): Promise<{ ok: boolean }> {
  return sdk(postApiAgentByIdAvatar({ path: { id: agentId }, body: file }))
}

/** Build the URL to an agent's avatar image (for use as `<img src>`). */
export function avatarUrl(agentId: string, cacheBust?: number): string {
  const base = (import.meta.env["VITE_API_URL"] as string | undefined) || ""
  const v = cacheBust ? `?v=${cacheBust}` : ""
  return `${base}/api/agent/${agentId}/avatar${v}`
}

// ── Tools / Radar / Entities (SDK) ────────────────────────────────────

export function fetchMetrics(agentId: string): Promise<AgentMetrics> {
  return sdk(getApiAgentByIdMetrics({ path: { id: agentId } }))
}

export function fetchVitals(agentId: string): Promise<Vital[]> {
  return sdk(getApiAgentByIdVitals({ path: { id: agentId } }))
}

export function fetchFleetMetrics(): Promise<AgentMetrics[]> {
  return sdk(getApiMetrics())
}

// ── Usage / Library (SDK) ─────────────────────────────────────────────

export function fetchLibrary(agentId: string): Promise<LibraryItem[]> {
  return sdk(getApiAgentByIdLibrary({ path: { id: agentId } }))
}

// ── Providers (SDK) ───────────────────────────────────────────────────

/** Fetch the raw provider registry from `GET /api/providers`. `allowedOnly`
 *  applies the org model allowlist server-side (`?allowed=1`). Returns the
 *  generated `ProviderDef` shape; frontend icon decoration lives in
 *  lib/support/models. This is the api-layer boundary — the only value import
 *  of the release/provider generated SDK (M141). */
export function fetchProviderDefs(
  allowedOnly = false,
): Promise<import("./generated/types.gen").ProviderDef[]> {
  return sdk(getApiProviders(allowedOnly ? { query: { allowed: "1" } } : {}))
}

// ── Auto-update (SDK, can_manage_it — O5.1, update-policy §5.9) ───────
//
// Wraps the generated auto-update SDK so the Update pane imports from
// `@/lib/api` (the api layer) instead of reaching into `@/lib/api/generated`
// directly — the M141 layering guard (only lib/api may value-import the
// generated SDK). The legacy manual release routes (download/select/delete)
// are retired server-side (T5.1.5) and have no client any more.

export type { UpdateStatus, UpdateWindow, UpdateApplyResponse } from "./generated/types.gen"

export function fetchUpdateStatus(): Promise<import("./generated/types.gen").UpdateStatus> {
  return sdk(getApiUpdateStatus())
}

export function checkForUpdate(): Promise<import("./generated/types.gen").UpdateStatus> {
  return sdk(postApiUpdateCheck())
}

export function applyUpdate(): Promise<import("./generated/types.gen").UpdateApplyResponse> {
  return sdk(postApiUpdateApply())
}

export function setUpdateMode(body: {
  mode?: "auto" | "manual" | "paused"
  window?: import("./generated/types.gen").UpdateWindow
}): Promise<import("./generated/types.gen").UpdateStatus> {
  return sdk(putApiUpdateMode({ body }))
}

// ── Commands (SDK) ────────────────────────────────────────────────────

export type CommandReceipt = GenCommandReceipt

export async function sendCommand(
  agentId: string,
  kind: Record<string, unknown>,
): Promise<CommandReceipt> {
  return sdk(
    postApiAgentByIdCommand({
      path: { id: agentId },
      body: buildCommandEnvelope(kind) as Record<string, unknown>,
    }),
  )
}

export function createCommand(
  agentId: string,
  cmd: { name: string; description?: string; body: string },
): Promise<CreateCommandReceipt> {
  return sdk(postApiAgentByIdLibraryCommand({ path: { id: agentId }, body: cmd }))
}

// ── Ticket (SDK) ──────────────────────────────────────────────────────

export async function mintTicket(): Promise<string> {
  const res = await sdk<{ ticket: string }>(postApiTicket())
  return res.ticket
}

// ── Threads (SDK + thin presentation wrapper) ────────────────────────

/** Format an epoch-ms timestamp as a relative age string. */
export function formatAge(epochMs: number): string {
  const delta = Date.now() - epochMs
  if (delta < 60_000) return "just now"
  const mins = Math.floor(delta / 60_000)
  if (mins < 60) return `${mins}m ago`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ago`
  const days = Math.floor(hrs / 24)
  return `${days}d ago`
}

export async function fetchThreads(agentId: string): Promise<ThreadDetail[]> {
  const res = await sdk<{ focusedThreadId?: string | null; threads: ThreadDetail[] }>(
    getApiAgentByIdThreads({ path: { id: agentId } }),
  )
  const focusedId = res.focusedThreadId ?? null
  return res.threads.map((t) => {
    // The generated type declares `lastActivity: string`, but the backend sends
    // an epoch number on the REST path and a display string on the SSE path.
    // Widen once (single assertion, no `unknown`) so the typeof-branch narrows
    // cleanly instead of needing a double-assertion at each read.
    const la = t.lastActivity as string | number
    return {
      ...t,
      agentId: t.agentId,
      lastActivity: typeof la === "number" ? formatAge(la) : la,
      lastActivityMs:
        typeof t.lastActivityMs === "number" ? t.lastActivityMs : typeof la === "number" ? la : 0,
      focused: focusedId != null && t.id === focusedId,
      log: t.log,
    }
  })
}

// ── Claude Code usage (SDK) ───────────────────────────────────────────

export function fetchClaudeUsage(): Promise<ClaudeUsageResponse> {
  return sdk(getApiClaudeUsage())
}

export function fetchClaudeTokenStatus(): Promise<
  import("./generated/types.gen").ClaudeTokenStatus
> {
  return sdk(getApiClaudeLoginStatus())
}

export function startClaudeLogin(): Promise<
  import("./generated/types.gen").ClaudeLoginStartResponse
> {
  return sdk(postApiClaudeLoginStart())
}

export function completeClaudeLogin(
  code: string,
): Promise<import("./generated/types.gen").ClaudeLoginCompleteResponse> {
  return sdk(postApiClaudeLoginComplete({ body: { code } }))
}

export function refreshClaudeLogin(): Promise<
  import("./generated/types.gen").ClaudeLoginCompleteResponse
> {
  return sdk(postApiClaudeLoginRefresh())
}

// ── Claude multi-account token vault (SDK) ────────────────────────────

export function fetchClaudeAccounts(): Promise<
  import("./generated/types.gen").ClaudeAccountsListResponse
> {
  return sdk(getApiClaudeAccounts())
}

export function storeClaudeAccount(): Promise<{ ok: boolean; email: string }> {
  return sdk(postApiClaudeAccountsStore())
}

export function switchClaudeAccount(email: string): Promise<{ ok: boolean; email: string }> {
  return sdk(postApiClaudeAccountsSwitch({ body: { email } }))
}

export function deleteClaudeAccount(email: string): Promise<{ ok: boolean }> {
  return sdk(deleteApiClaudeAccountsByEmail({ path: { email } }))
}
