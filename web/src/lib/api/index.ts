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

import type { Fleet, AgentDetail } from "../types"
import type {
  FleetMetrics,
  CommandReceipt as GenCommandReceipt,
  CreateFleetReceipt,
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
  getApiFleetsMeta,
  getApiFleetsRetired,
  getApiFleetByIdMeta,
  getApiFleetByIdMetrics,
  getApiFleetByIdVitals,
  getApiFleetByIdLibrary,
  getApiFleetByIdLibrarySystemByItem,
  putApiFleetByIdLibrarySystemByItem,
  deleteApiFleetByIdLibrarySystemByItem,
  getApiFleetByIdAgents,
  getApiProviders,
  getApiUpdateStatus,
  postApiUpdateCheck,
  postApiUpdateApply,
  putApiUpdateMode,
  getApiMetrics,
  postApiFleetsCreate,
  postApiFleetByIdRestart,
  postApiFleetByIdRetire,
  postApiFleetByIdUnretire,
  postApiFleetByIdRename,
  postApiFleetByIdCommand,
  postApiFleetByIdLibraryCommand,
  putApiFleetByIdLibraryCommandByItem,
  postApiFleetByIdConversationsSearch,
  postApiFleetByIdSql,
  postApiTicket,
} from "./generated"
import { buildCommandEnvelope, sdk } from "./client"
import { sendUpload } from "./client/upload"

export { getToken, setToken } from "./client"
export * from "./client/errors"
export * from "./auth"
export * from "./finder"
export * from "./body"
export * from "./envKeys"
export * from "./it"

// ── Type re-exports ───────────────────────────────────────────────────

export type { CreateFleetReceipt } from "./generated/types.gen"
export type { RestartReceipt } from "./generated/types.gen"
export type { RetireReceipt, UnretireReceipt } from "./generated/types.gen"
export type { FleetMetrics } from "./generated/types.gen"
export type { Vital } from "./generated/types.gen"
export type { CreateCommandReceipt } from "./generated/types.gen"
export type { ClaudeUsageResponse, ClaudeUsageLimit } from "./generated/types.gen"
export type {
  ClaudeTokenStatus,
  ClaudeLoginStartResponse,
  ClaudeLoginCompleteResponse,
} from "./generated/types.gen"
export type { ClaudeAccountSummary, ClaudeAccountsListResponse } from "./generated/types.gen"
export type { ConvHit } from "./generated/types.gen"

// ── Helper: align TS with runtime (setupClient.ts guarantees) ─────────

// ── Fleet (SDK) ───────────────────────────────────────────────────────

export function fetchFleet(): Promise<Fleet[]> {
  return sdk(getApiFleetsMeta())
}

export function fetchRetiredFleet(): Promise<Fleet[]> {
  return sdk(getApiFleetsRetired())
}

// ── Agent lifecycle (SDK) ─────────────────────────────────────────────

export function createFleet(body: {
  name: string
  folder?: string
  model?: string
}): Promise<CreateFleetReceipt> {
  return sdk(postApiFleetsCreate({ body }))
}

export function restartFleet(fleetId: string): Promise<RestartReceipt> {
  return sdk(postApiFleetByIdRestart({ path: { id: fleetId } }))
}

export function retireFleet(fleetId: string): Promise<RetireReceipt> {
  return sdk(postApiFleetByIdRetire({ path: { id: fleetId } }))
}

export function unretireFleet(fleetId: string): Promise<UnretireReceipt> {
  return sdk(postApiFleetByIdUnretire({ path: { id: fleetId } }))
}

export function renameFleet(fleetId: string, name: string): Promise<{ ok: boolean }> {
  return sdk(postApiFleetByIdRename({ path: { id: fleetId }, body: { name } }))
}

// ── Agent meta (SDK) ──────────────────────────────────────────────────

export function fetchFleetMeta(fleetId: string): Promise<Fleet> {
  return sdk(getApiFleetByIdMeta({ path: { id: fleetId } }))
}

// ── Agent avatar ──────────────────────────────────────────────────────

export function uploadAvatar(fleetId: string, file: File): Promise<{ ok: boolean }> {
  return sendUpload<{ ok: boolean }>(`/api/fleet/${fleetId}/avatar`, file, file.name || "avatar")
}

/** Build the URL to an agent's avatar image (for use as `<img src>`). */
export function avatarUrl(fleetId: string, cacheBust?: number): string {
  const base = (import.meta.env["VITE_API_URL"] as string | undefined) || ""
  const v = cacheBust ? `?v=${cacheBust}` : ""
  return `${base}/api/fleet/${fleetId}/avatar${v}`
}

// ── Tools / Radar / Entities (SDK) ────────────────────────────────────

export function fetchMetrics(fleetId: string): Promise<FleetMetrics> {
  return sdk(getApiFleetByIdMetrics({ path: { id: fleetId } }))
}

export function fetchVitals(fleetId: string): Promise<Vital[]> {
  return sdk(getApiFleetByIdVitals({ path: { id: fleetId } }))
}

export function fetchFleetMetrics(): Promise<FleetMetrics[]> {
  return sdk(getApiMetrics())
}

// ── Usage / Library (SDK) ─────────────────────────────────────────────

export function fetchLibrary(fleetId: string): Promise<LibraryItem[]> {
  return sdk(getApiFleetByIdLibrary({ path: { id: fleetId } }))
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
  channel?: "stable" | "nightly"
  window?: import("./generated/types.gen").UpdateWindow
}): Promise<import("./generated/types.gen").UpdateStatus> {
  return sdk(putApiUpdateMode({ body }))
}

// ── Commands (SDK) ────────────────────────────────────────────────────

export type CommandReceipt = GenCommandReceipt

export async function sendCommand(
  fleetId: string,
  kind: Record<string, unknown>,
): Promise<CommandReceipt> {
  return sdk(
    postApiFleetByIdCommand({
      path: { id: fleetId },
      body: buildCommandEnvelope(kind) as Record<string, unknown>,
    }),
  )
}

export function createCommand(
  fleetId: string,
  cmd: { name: string; description?: string; body: string },
): Promise<CreateCommandReceipt> {
  return sdk(postApiFleetByIdLibraryCommand({ path: { id: fleetId }, body: cmd }))
}

/** Create or overwrite a `/command` `.md` by its file id (the bubble row's
 *  per-command Edit button). The command twin of {@link upsertLibrarySystem}:
 *  `PUT …/library/command/{item}` deliberately overwrites, unlike the 409-on-
 *  clobber {@link createCommand}. */
export function updateCommand(
  fleetId: string,
  itemId: string,
  cmd: { name: string; description?: string; body: string },
): Promise<CreateCommandReceipt> {
  return sdk(
    putApiFleetByIdLibraryCommandByItem({ path: { id: fleetId, item: itemId }, body: cmd }),
  )
}

// ── Agent library CRUD (T581 footer editor) ───────────────────────────

export type LibrarySystemRaw = import("./generated/types.gen").LibrarySystemRaw

/** Read one behaviour agent's raw authoring fields (Export + Edit-prefill). */
export function fetchLibrarySystem(fleetId: string, itemId: string): Promise<LibrarySystemRaw> {
  return sdk(getApiFleetByIdLibrarySystemByItem({ path: { id: fleetId, item: itemId } }))
}

/** Create or overwrite a behaviour agent `.md` (user agent or built-in override). */
export function upsertLibrarySystem(
  fleetId: string,
  itemId: string,
  agent: { name: string; description?: string; body: string },
): Promise<CreateCommandReceipt> {
  return sdk(
    putApiFleetByIdLibrarySystemByItem({ path: { id: fleetId, item: itemId }, body: agent }),
  )
}

/** Delete a behaviour agent's on-disk `.md` (override revert, or user agent removal). */
export function deleteLibrarySystem(
  fleetId: string,
  itemId: string,
): Promise<CreateCommandReceipt> {
  return sdk(deleteApiFleetByIdLibrarySystemByItem({ path: { id: fleetId, item: itemId } }))
}

// ── Ticket (SDK) ──────────────────────────────────────────────────────

export async function mintTicket(): Promise<string> {
  const res = await sdk<{ ticket: string }>(postApiTicket())
  return res.ticket
}

// ── Agents (SDK + thin presentation wrapper) ────────────────────────

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

export async function fetchAgents(fleetId: string): Promise<AgentDetail[]> {
  const res = await sdk<{ focusedAgentId?: string | null; agents: AgentDetail[] }>(
    getApiFleetByIdAgents({ path: { id: fleetId } }),
  )
  const focusedId = res.focusedAgentId ?? null
  return res.agents.map((t) => {
    // The generated type declares `lastActivity: string`, but the backend sends
    // an epoch number on the REST path and a display string on the SSE path.
    // Widen once (single assertion, no `unknown`) so the typeof-branch narrows
    // cleanly instead of needing a double-assertion at each read.
    const la = t.lastActivity as string | number
    return {
      ...t,
      lastActivity: typeof la === "number" ? formatAge(la) : la,
      lastActivityMs:
        typeof t.lastActivityMs === "number" ? t.lastActivityMs : typeof la === "number" ? la : 0,
      focused: focusedId != null && t.id === focusedId,
      log: t.log,
    }
  })
}

// ── Conversation search (T671, hybrid keyword+semantic) ──────────────

/**
 * Hybrid (keyword + semantic) search over an agent's conversation index.
 *
 * Forwards to the agent over the bridge; Meilisearch embeds the raw query
 * server-side, so no fabricated semantic query is needed. `agentId` scopes
 * the search to one agent (omit for realm-wide). Returns hits best-scored
 * first; an empty array means no match.
 */
export function searchConversations(
  fleetId: string,
  query: string,
  opts?: { limit?: number; agentId?: string },
): Promise<import("./generated/types.gen").ConvHit[]> {
  return sdk<{ hits: import("./generated/types.gen").ConvHit[] }>(
    postApiFleetByIdConversationsSearch({
      path: { id: fleetId },
      body: {
        query,
        ...(opts?.limit != null && { limit: opts.limit }),
        ...(opts?.agentId != null && { agent_id: opts.agentId }),
      },
    }),
  ).then((r) => r.hits)
}

/** One page of a read-only entity SQL query, answered by the agent (openui `SqlTable`). */
export function fetchSqlPage(
  fleetId: string,
  body: {
    sql: string
    offset: number
    limit: number
    view?: import("./generated/types.gen").SqlView
  },
): Promise<import("./generated/types.gen").SqlPage> {
  return sdk<import("./generated/types.gen").SqlPage>(
    postApiFleetByIdSql({ path: { id: fleetId }, body }),
  )
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
