// ── REST API client for the orchestration backend ────────────────────
//
// Two-layer barrel: endpoints whose backend response matches the OpenAPI
// spec call the generated SDK directly (thin 1-line wrappers); endpoints
// whose backend serialisation still diverges from the spec keep a manual
// `request()` with a transformation.  The sub-modules (auth, finder, body,
// env-keys) are re-exported so `@/lib/api` remains the single import
// surface.
//
// NOTE: setupClient.ts configures the hey-api singleton with
// `throwOnError: true` + `responseStyle: 'data'`, so SDK calls return
// data directly and throw on non-2xx.  TypeScript generics default to
// `ThrowOnError = false`, hence the `as` casts below — they align the
// compile-time type with the runtime guarantee.

import type {
  Agent,
  ContextPanel,
  ThreadDetail,
} from "../types"
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
} from "./generated/types.gen"
import {
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
import { request, buildCommandEnvelope } from "./client"

export { getToken, setToken } from "./client"
export * from "./auth"
export * from "./finder"
export * from "./body"
export * from "./env-keys"

// ── Type re-exports ───────────────────────────────────────────────────

export type { CreateAgentReceipt } from "./generated/types.gen"
export type { RestartReceipt } from "./generated/types.gen"
export type { RetireReceipt, UnretireReceipt } from "./generated/types.gen"
export type { RadarData } from "./generated/types.gen"
export type { AgentMetrics } from "./generated/types.gen"
export type { Vital } from "./generated/types.gen"
export type { CreateCommandReceipt } from "./generated/types.gen"

// ── Helper: align TS with runtime (setupClient.ts guarantees) ─────────

/** SDK calls return `T` at runtime (throwOnError + responseStyle:'data'),
 *  but the generic defaults produce a wider type.  This cast is safe. */
function sdk<T>(call: unknown): Promise<T> {
  return call as Promise<T>
}

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

export function renameAgent(
  agentId: string,
  name: string,
): Promise<{ ok: boolean }> {
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
  const base = import.meta.env.VITE_API_URL || ""
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

// ── Commands (SDK) ────────────────────────────────────────────────────

export type CommandReceipt = GenCommandReceipt

export async function sendCommand(
  agentId: string,
  kind: Record<string, unknown>,
): Promise<CommandReceipt> {
  return sdk(postApiAgentByIdCommand({
    path: { id: agentId },
    body: buildCommandEnvelope(kind) as Record<string, unknown>,
  }))
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

// ═══════════════════════════════════════════════════════════════════════
// Endpoints below keep manual request() because the backend response
// format diverges from the OpenAPI spec (raw YAML dumps or wrapped
// objects).  TODO: fix backend handlers to return spec-compliant shapes,
// then migrate these to SDK calls.
// ═══════════════════════════════════════════════════════════════════════

// ── Threads (manual — backend field names differ from spec) ───────────

/** Format an epoch-ms timestamp as a relative age string. */
function formatAge(epochMs: number): string {
  const delta = Date.now() - epochMs
  if (delta < 60_000) return "just now"
  const mins = Math.floor(delta / 60_000)
  if (mins < 60) return `${mins}m ago`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ago`
  const days = Math.floor(hrs / 24)
  return `${days}d ago`
}

interface RawMsg {
  id: string
  role: string
  content?: string
  timestamp?: number
  text?: string
  author?: string
  ts?: string
  tool?: unknown
  questions?: unknown
  fileRef?: string
  auto?: boolean
}

interface RawThread {
  id: string
  name: string
  status: string
  agentId: string
  lastActivity: number | string
  lastMessage?: string
  messageCount?: number
  unread?: number
  archived?: boolean
  paused?: boolean
  log?: RawMsg[]
  agent?: string
  createdAt?: string
}

interface ThreadsResponse {
  focusedThreadId: string | null
  threads: RawThread[]
}

/** Map backend question JSON to frontend ThreadQuestion shape. */
export function mapRawQuestions(raw: unknown): ThreadDetail["log"][number]["questions"] {
  if (!raw) return undefined
  let arr = Array.isArray(raw) ? raw : [raw]
  if (arr.length === 1 && Array.isArray(arr[0])) arr = arr[0]
  return arr.map((q: Record<string, unknown>) => ({
    header: (q.header as string) ?? undefined,
    prompt: (q.question as string) ?? (q.prompt as string) ?? "",
    options: Array.isArray(q.options)
      ? q.options.map((o: unknown) =>
          typeof o === "string" ? o : (o as Record<string, string>)?.label ?? "")
      : [],
    multi: (q.multiSelect as boolean) ?? (q.multi as boolean) ?? false,
    allowOther: (q.allowOther as boolean) ?? false,
  }))
}

export function fetchThreads(agentId: string): Promise<ThreadDetail[]> {
  return request<ThreadsResponse | RawThread[]>(`/api/agent/${agentId}/threads`).then((raw) => {
    const focusedId = Array.isArray(raw) ? null : raw.focusedThreadId
    const list: RawThread[] = Array.isArray(raw) ? raw : (raw.threads ?? [])
    return list.map((t) => ({
      id: t.id,
      name: t.name,
      status: (t.status === "MyTurn" ? "MY_TURN" : t.status === "TheirTurn" ? "THEIR_TURN" : t.status) as ThreadDetail["status"],
      agentId: t.agentId ?? agentId,
      agent: t.agent ?? agentId,
      createdAt: t.createdAt ?? (typeof t.lastActivity === "number" ? new Date(t.lastActivity).toISOString() : t.lastActivity),
      lastActivity: typeof t.lastActivity === "number" ? formatAge(t.lastActivity) : t.lastActivity,
      lastActivityMs: typeof t.lastActivity === "number" ? t.lastActivity : 0,
      unread: t.unread ?? 0,
      archived: t.archived ?? false,
      paused: t.paused ?? false,
      focused: focusedId != null && t.id === focusedId,
      log: (t.log ?? []).map((m) => ({
        id: m.id,
        author: (m.author ?? m.role ?? "user") as "user" | "assistant",
        text: m.text ?? m.content,
        ts: m.ts ?? (m.timestamp ? new Date(m.timestamp).toISOString() : ""),
        tool: m.tool as ThreadDetail["log"][number]["tool"],
        questions: mapRawQuestions(m.questions),
        fileRef: m.fileRef,
        auto: m.auto,
      })),
    }))
  })
}

// ── Memory (manual — backend sends raw YAML map, not MemoryCard[]) ───

export function fetchMemory(agentId: string): Promise<MemoryCard[]> {
  return request<Record<string, Record<string, unknown>>>(`/api/agent/${agentId}/memory`).then((raw) => {
    if (Array.isArray(raw)) return raw as unknown as MemoryCard[]
    return Object.entries(raw).map(([_id, m], i) => ({
      id: `M${i + 1}`,
      tldr: (m.tl_dr ?? "") as string,
      importance: (m.importance ?? "medium") as MemoryCard["importance"],
      labels: (m.labels ?? []) as string[],
    }))
  })
}

// ── Todos (manual — backend wraps in {todos: [...]}) ──────────────────

export function fetchTodos(agentId: string): Promise<TodoItem[]> {
  return request<Record<string, unknown>>(`/api/agent/${agentId}/todos`).then((raw) => {
    if (Array.isArray(raw)) return raw as TodoItem[]
    const todos = (raw.todos ?? []) as Array<Record<string, unknown>>
    return todos.map((t) => ({
      id: t.id as string,
      name: t.name as string,
      status: (t.status ?? "pending") as TodoItem["status"],
      depth: (t.depth as number) ?? 0,
    }))
  })
}

// ── Spine (manual — backend wraps in {notifications: [...]}) ──────────

export function fetchSpine(agentId: string): Promise<SpineNotif[]> {
  return request<Record<string, unknown>>(`/api/agent/${agentId}/spine`).then((raw) => {
    if (Array.isArray(raw)) return raw as SpineNotif[]
    const notifs = (raw.notifications ?? []) as Array<Record<string, unknown>>
    return notifs.map((n) => ({
      id: n.id as string,
      kind: (n.notification_type ?? "custom") as SpineNotif["kind"],
      time: n.timestamp_ms ? new Date(n.timestamp_ms as number).toISOString() : "",
      text: (n.content ?? "") as string,
      processed: n.status === "processed",
    }))
  })
}

// ── Queue (manual — backend wraps in {queued_calls: [...]}) ───────────

export function fetchQueue(agentId: string): Promise<QueueAction[]> {
  return request<Record<string, unknown>>(`/api/agent/${agentId}/queue`).then((raw) => {
    if (Array.isArray(raw)) return raw as QueueAction[]
    return (raw.queued_calls ?? []) as QueueAction[]
  })
}

// ── Scratchpad (manual — backend wraps in {scratchpad_cells: [...]}) ──

export function fetchScratchpad(agentId: string): Promise<ScratchCell[]> {
  return request<Record<string, unknown>>(`/api/agent/${agentId}/scratchpad`).then((raw) => {
    if (Array.isArray(raw)) return raw as ScratchCell[]
    const cells = (raw.scratchpad_cells ?? []) as Array<Record<string, unknown>>
    return cells.map((c) => ({
      id: (c.id ?? "") as string,
      title: (c.title ?? "") as string,
      preview: ((c.content ?? "") as string).slice(0, 200),
    }))
  })
}

// ── Tree (manual — backend sends raw YAML map, not TreeRow[]) ─────────

export function fetchTree(agentId: string): Promise<TreeRow[]> {
  return request<Record<string, Record<string, unknown>>>(`/api/agent/${agentId}/tree`).then((raw) => {
    if (Array.isArray(raw)) return raw as unknown as TreeRow[]
    return Object.values(raw).map((t) => ({
      depth: 0,
      name: ((t.path as string) ?? "").split("/").pop() ?? "",
      kind: "file" as const,
      desc: (t.description ?? "") as string,
      changed: !!t.changed,
    }))
  })
}

// ── Callbacks (manual — backend sends raw YAML map, not CallbackRow[])─

export function fetchCallbacks(agentId: string): Promise<CallbackRow[]> {
  return request<Record<string, Record<string, unknown>>>(`/api/agent/${agentId}/callbacks`).then((raw) => {
    if (Array.isArray(raw)) return raw as unknown as CallbackRow[]
    return Object.entries(raw).map(([id, c]) => ({
      id,
      name: (c.name ?? id) as string,
      pattern: (c.pattern ?? "") as string,
      blocking: !!c.blocking,
      timeout: c.timeout ? `${c.timeout}s` : "",
      scope: c.is_global ? "global" : "local",
      cwd: (c.cwd ?? "") as string,
    }))
  })
}
