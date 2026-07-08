// ── REST API client for the orchestration backend ────────────────────
//
// Every endpoint uses the generated SDK from openapi.json — zero manual
// fetch calls.  Thin presentation wrappers (formatAge, mapRawQuestions)
// reshape backend data for UI consumption; the API contract itself is
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

import type { Agent, ContextPanel, ThreadDetail } from "../types"
import type {
  AgentMetrics,
  CallbackRow,
  CommandReceipt as GenCommandReceipt,
  CreateAgentReceipt,
  CreateCommandReceipt,
  EntityTable,
  LibraryItem,
  MemoryCard,
  QueueAction,
  RadarData,
  RestartReceipt,
  RetireReceipt,
  ScratchCell,
  SpineNotif,
  TodoItem,
  ToolGroup,
  TreeRow,
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
  getApiFleetMeta,
  getApiFleetRetired,
  getApiAgentByIdMeta,
  getApiAgentByIdPanels,
  getApiAgentByIdTools,
  getApiAgentByIdRadar,
  getApiAgentByIdEntities,
  getApiAgentByIdMetrics,
  getApiAgentByIdVitals,
  getApiAgentByIdLibrary,
  getApiAgentByIdUsage,
  getApiAgentByIdThreads,
  getApiAgentByIdMemory,
  getApiAgentByIdTodos,
  getApiAgentByIdSpine,
  getApiAgentByIdQueue,
  getApiAgentByIdScratchpad,
  getApiAgentByIdTree,
  getApiAgentByIdCallbacks,
  getApiProviders,
  getApiReleases,
  putApiReleasesArch,
  postApiReleasesDownload,
  putApiReleasesSelect,
  deleteApiReleasesByTag,
  postApiReleasesDeploy,
  postApiReleasesRestartOrchestrator,
  getApiMetrics,
  postApiFleetCreate,
  postApiAgentByIdRestart,
  postApiAgentByIdRetire,
  postApiAgentByIdUnretire,
  postApiAgentByIdRename,
  postApiAgentByIdAvatar,
  deleteApiAgentByIdAvatar,
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

// ── Type re-exports ───────────────────────────────────────────────────

export type { CreateAgentReceipt } from "./generated/types.gen"
export type { RestartReceipt } from "./generated/types.gen"
export type { RetireReceipt, UnretireReceipt } from "./generated/types.gen"
export type { RadarData } from "./generated/types.gen"
export type { AgentMetrics } from "./generated/types.gen"
export type { Vital } from "./generated/types.gen"
export type { CreateCommandReceipt } from "./generated/types.gen"
export type { ClaudeUsageResponse, ClaudeUsageLimit } from "./generated/types.gen"
export type {
  ClaudeTokenStatus,
  ClaudeLoginStartResponse,
  ClaudeLoginCompleteResponse,
} from "./generated/types.gen"

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

export function deleteAvatar(agentId: string): Promise<{ ok: boolean }> {
  return sdk(deleteApiAgentByIdAvatar({ path: { id: agentId } }))
}

/** Build the URL to an agent's avatar image (for use as `<img src>`). */
export function avatarUrl(agentId: string, cacheBust?: number): string {
  const base = (import.meta.env["VITE_API_URL"] as string | undefined) || ""
  const v = cacheBust ? `?v=${cacheBust}` : ""
  return `${base}/api/agent/${agentId}/avatar${v}`
}

// ── Panels (SDK) ──────────────────────────────────────────────────────

export function fetchPanels(agentId: string): Promise<ContextPanel[]> {
  return sdk(getApiAgentByIdPanels({ path: { id: agentId } }))
}

// ── Tools / Radar / Entities (SDK) ────────────────────────────────────

export function fetchTools(agentId: string): Promise<ToolGroup[]> {
  return sdk(getApiAgentByIdTools({ path: { id: agentId } }))
}

export function fetchRadar(agentId: string): Promise<RadarData> {
  return sdk(getApiAgentByIdRadar({ path: { id: agentId } }))
}

export function fetchEntities(agentId: string): Promise<EntityTable[]> {
  return sdk(getApiAgentByIdEntities({ path: { id: agentId } }))
}

// ── Metrics / Vitals (SDK) ────────────────────────────────────────────

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

export function fetchUsage(agentId: string): Promise<Record<string, unknown>> {
  return sdk(getApiAgentByIdUsage({ path: { id: agentId } }))
}

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

// ── Releases (SDK, admin-only T427) ───────────────────────────────────
//
// Wraps the generated release-lifecycle SDK so the shell/config release
// panes import from `@/lib/api` (the api layer) instead of reaching into
// `@/lib/api/generated` directly — the M141 layering guard (only lib/api
// may value-import the generated SDK).

export type { ReleaseEntry, ReleasesResponse, DeployResponse } from "./generated/types.gen"

export function fetchReleases(): Promise<import("./generated/types.gen").ReleasesResponse> {
  return sdk(getApiReleases())
}

export function setReleasesArch(body: { arch?: string; auto?: boolean }): Promise<unknown> {
  return sdk(putApiReleasesArch({ body }))
}

export function downloadRelease(tag: string): Promise<unknown> {
  return sdk(postApiReleasesDownload({ body: { tag } }))
}

export function selectRelease(tag: string): Promise<unknown> {
  return sdk(putApiReleasesSelect({ body: { tag } }))
}

export function deleteRelease(tag: string): Promise<unknown> {
  return sdk(deleteApiReleasesByTag({ path: { tag } }))
}

export function deployFleet(
  tag: string | null,
): Promise<import("./generated/types.gen").DeployResponse> {
  return sdk(postApiReleasesDeploy({ body: tag ? { tag } : {} }))
}

export function restartOrchestrator(): Promise<unknown> {
  return sdk(postApiReleasesRestartOrchestrator())
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

/** Map backend question JSON to frontend ThreadQuestion shape. */
export function mapRawQuestions(raw: unknown): ThreadDetail["log"][number]["questions"] {
  if (!raw) return undefined
  let arr = Array.isArray(raw) ? raw : [raw]
  if (arr.length === 1 && Array.isArray(arr[0])) arr = arr[0]
  return arr.map((q: Record<string, unknown>) => {
    // `header` is optional on ThreadQuestion (`header?: string`); under
    // exactOptionalPropertyTypes an explicit `header: undefined` is NOT
    // assignable to that slot, so OMIT it when absent via a conditional spread
    // rather than writing `undefined` into it.
    const header = q["header"] as string | undefined
    return {
      ...(header !== undefined && { header }),
      prompt: (q["question"] as string | undefined) ?? (q["prompt"] as string | undefined) ?? "",
      options: Array.isArray(q["options"])
        ? q["options"].map((o: unknown) =>
            typeof o === "string"
              ? o
              : ((o as Record<string, string> | undefined)?.["label"] ?? ""),
          )
        : [],
      multi:
        (q["multiSelect"] as boolean | undefined) ?? (q["multi"] as boolean | undefined) ?? false,
      allowOther: (q["allowOther"] as boolean | undefined) ?? false,
    }
  })
}

export async function fetchThreads(agentId: string): Promise<ThreadDetail[]> {
  const res = await sdk<{ focusedThreadId?: string | null; threads: ThreadDetail[] }>(
    getApiAgentByIdThreads({ path: { id: agentId } }),
  )
  const focusedId = res.focusedThreadId ?? null
  return res.threads.map((t) => ({
    ...t,
    agentId: t.agentId,
    lastActivity:
      typeof t.lastActivity === "number"
        ? formatAge(t.lastActivity as unknown as number)
        : (t.lastActivity ?? ""),
    lastActivityMs:
      typeof t.lastActivityMs === "number"
        ? t.lastActivityMs
        : typeof t.lastActivity === "number"
          ? (t.lastActivity as unknown as number)
          : 0,
    focused: focusedId != null && t.id === focusedId,
    log: t.log.map((m) => ({
      ...m,
      questions: mapRawQuestions(m.questions),
    })),
  }))
}

// ── Memory (SDK) ──────────────────────────────────────────────────────

export function fetchMemory(agentId: string): Promise<MemoryCard[]> {
  return sdk(getApiAgentByIdMemory({ path: { id: agentId } }))
}

// ── Todos (SDK) ───────────────────────────────────────────────────────

export function fetchTodos(agentId: string): Promise<TodoItem[]> {
  return sdk(getApiAgentByIdTodos({ path: { id: agentId } }))
}

// ── Spine (SDK) ───────────────────────────────────────────────────────

export function fetchSpine(agentId: string): Promise<SpineNotif[]> {
  return sdk(getApiAgentByIdSpine({ path: { id: agentId } }))
}

// ── Queue (SDK) ───────────────────────────────────────────────────────

export function fetchQueue(agentId: string): Promise<QueueAction[]> {
  return sdk(getApiAgentByIdQueue({ path: { id: agentId } }))
}

// ── Scratchpad (SDK) ──────────────────────────────────────────────────

export function fetchScratchpad(agentId: string): Promise<ScratchCell[]> {
  return sdk(getApiAgentByIdScratchpad({ path: { id: agentId } }))
}

// ── Tree (SDK) ────────────────────────────────────────────────────────

export function fetchTree(agentId: string): Promise<TreeRow[]> {
  return sdk(getApiAgentByIdTree({ path: { id: agentId } }))
}

// ── Callbacks (SDK) ───────────────────────────────────────────────────

export function fetchCallbacks(agentId: string): Promise<CallbackRow[]> {
  return sdk(getApiAgentByIdCallbacks({ path: { id: agentId } }))
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
