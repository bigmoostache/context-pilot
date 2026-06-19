// ── REST API client for the orchestration backend ────────────────────
//
// Base URL from env (VITE_API_URL) with fallback to localhost:7878.
// Every function returns typed JSON or throws on non-2xx.

const BASE = import.meta.env.VITE_API_URL ?? "http://localhost:7878"

/** Typed fetch wrapper — throws on non-2xx with the response body. */
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, init)
  if (!res.ok) {
    const body = await res.text().catch(() => res.statusText)
    throw new Error(`${res.status} ${path}: ${body}`)
  }
  return res.json() as Promise<T>
}

// ── Fleet ─────────────────────────────────────────────────────────────

import type {
  Agent,
  ContextPanel,
  MemoryCard,
  TodoItem,
  SpineNotif,
  QueueAction,
  ScratchCell,
  TreeRow,
  CallbackRow,
  ToolGroup,
  RadarAnchor,
  RadarResult,
  EntityTable,
  ThreadDetail,
  LibraryItem,
} from "./types"

export function fetchFleet(): Promise<Agent[]> {
  return request("/api/fleet/meta")
}

// ── Create agent ──────────────────────────────────────────────────────

/** Receipt from `POST /api/fleet/create` — a 202 "spawning" acknowledgement.
 *  The agent self-registers and appears in the fleet within a scan tick once
 *  it has booted, so this is launch confirmation, not the agent itself. */
export interface CreateAgentReceipt {
  status: string
  folder: string
  pid: number
}

/** Create a new agent: the backend mkdir's its realm folder and spawns the
 *  `cp` TUI on a pty (so the full agent stack runs). `model` is accepted for
 *  forward-compat but not yet applied (the TUI has no `--model` flag). */
export function createAgent(body: {
  name: string
  folder?: string
  model?: string
}): Promise<CreateAgentReceipt> {
  return request("/api/fleet/create", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  })
}

/** Receipt from `POST /api/agent/{id}/restart` — a 202 "restarting"
 *  acknowledgement. The agent's old process is killed and a fresh one is
 *  spawned on the same realm folder (so it re-registers under the same id);
 *  it re-appears in the fleet within a scan tick once it has booted. */
export interface RestartReceipt {
  status: string
  folder: string
  pid: number
}

/** Restart an agent: kill its (possibly stale) running process and respawn it
 *  from the backend's current `cp` binary on the same realm folder. Used when
 *  an agent's running binary predates a command the cockpit wants to send and
 *  its bridge rejects it with `502 agent unreachable`. */
export function restartAgent(agentId: string): Promise<RestartReceipt> {
  return request(`/api/agent/${agentId}/restart`, { method: "POST" })
}

// ── Retire / unretire (T271) ──────────────────────────────────────────

/** Receipt from `POST /api/agent/{id}/retire` — the agent's process (and its
 *  console-server daemon) is stopped and the agent is recorded as retired; its
 *  realm folder is kept intact so it can be brought back. */
export interface RetireReceipt {
  status: string
  id: string
  folder: string
}

/** Receipt from `POST /api/agent/{id}/unretire` — a 202 "unretiring" launch
 *  acknowledgement; the agent respawns on its kept folder and re-registers
 *  under the same id within a scan tick. */
export interface UnretireReceipt {
  status: string
  id: string
  folder: string
  pid: number
}

/** Retire (archive) an agent: stop its process + console server, keep its
 *  folder, and move it to the Retired section. Not a delete — fully reversible
 *  via {@link unretireAgent}. */
export function retireAgent(agentId: string): Promise<RetireReceipt> {
  return request(`/api/agent/${agentId}/retire`, { method: "POST" })
}

/** Bring a retired agent back: clear its retired flag and respawn it on the
 *  same realm folder (re-registering under the same id). */
export function unretireAgent(agentId: string): Promise<UnretireReceipt> {
  return request(`/api/agent/${agentId}/unretire`, { method: "POST" })
}

/** Fetch the Retired section — one `Agent`-shaped record per retired agent,
 *  built from the orchestrator's retired store (the agents have no live process
 *  to inspect). Each carries `status: "retired"`. */
export function fetchRetiredFleet(): Promise<Agent[]> {
  return request("/api/fleet/retired")
}

// ── Agent meta ────────────────────────────────────────────────────────

export function fetchAgentMeta(agentId: string): Promise<Agent> {
  return request(`/api/agent/${agentId}/meta`)
}

// ── Threads ───────────────────────────────────────────────────────────

/** Format an epoch-ms timestamp as a relative age string ("just now", "3m ago", etc). */
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

/** Raw message shape from the backend (differs from maquette ThreadMsg). */
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

/** Raw thread shape from the backend. */
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
  log?: RawMsg[]
  // maquette fields (pass through if present)
  agent?: string
  createdAt?: string
}

/** Wrapper shape from the backend threads endpoint. */
interface ThreadsResponse {
  focusedThreadId: string | null
  threads: RawThread[]
}

export function fetchThreads(agentId: string): Promise<ThreadDetail[]> {
  return request<ThreadsResponse | RawThread[]>(`/api/agent/${agentId}/threads`).then((raw) => {
    // Handle both wrapper shape { focusedThreadId, threads } and legacy array
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
      focused: focusedId != null && t.id === focusedId,
      log: (t.log ?? []).map((m) => ({
        id: m.id,
        author: (m.author ?? m.role ?? "user") as "user" | "assistant",
        text: m.text ?? m.content,
        ts: m.ts ?? (m.timestamp ? new Date(m.timestamp).toISOString() : ""),
        tool: m.tool as ThreadDetail["log"][number]["tool"],
        questions: m.questions as ThreadDetail["log"][number]["questions"],
        fileRef: m.fileRef,
        auto: m.auto,
      })),
    }))
  })
}

// ── Panels ────────────────────────────────────────────────────────────

export function fetchPanels(agentId: string): Promise<ContextPanel[]> {
  return request(`/api/agent/${agentId}/panels`)
}

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

export function fetchQueue(agentId: string): Promise<QueueAction[]> {
  return request<Record<string, unknown>>(`/api/agent/${agentId}/queue`).then((raw) => {
    if (Array.isArray(raw)) return raw as QueueAction[]
    return (raw.queued_calls ?? []) as QueueAction[]
  })
}

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

export function fetchTools(agentId: string): Promise<ToolGroup[]> {
  return request(`/api/agent/${agentId}/tools`)
}

export interface RadarData {
  anchors: RadarAnchor[]
  results: RadarResult[]
}

export function fetchRadar(agentId: string): Promise<RadarData> {
  return request(`/api/agent/${agentId}/radar`)
}

export function fetchEntities(agentId: string): Promise<EntityTable[]> {
  return request(`/api/agent/${agentId}/entities`)
}

// ── Finder ────────────────────────────────────────────────────────────

import type { FinderNode } from "./types"

export function fetchFs(agentId: string, path = ""): Promise<FinderNode[]> {
  const q = path ? `?path=${encodeURIComponent(path)}` : ""
  return request(`/api/agent/${agentId}/fs${q}`)
}

/** A file content preview from `GET /api/agent/{id}/fs/preview?path=`.
 *
 * The backend returns the first 256 KiB of a text file (`truncated` flags the
 * cap) and rejects binary content with a 415 — so a thrown `fetchFsPreview`
 * means "no text preview for this file", which the Finder renders as the plain
 * "No preview available" state. */
export interface FsPreview {
  content: string
  size: number
  truncated: boolean
}

/** Fetch a file's text content for the Finder Quick Look pane. Throws on a
 *  binary file (415) or read fault — callers fall back to the no-preview
 *  state. */
export function fetchFsPreview(agentId: string, path: string): Promise<FsPreview> {
  return request(`/api/agent/${agentId}/fs/preview?path=${encodeURIComponent(path)}`)
}

/** Result of a single-file upload (`POST /fs/upload`). */
export interface UploadResult {
  written: number
  path: string
}

/** Upload one file into a directory of the agent's realm. The body is the
 *  file's raw bytes; `dir` is the realm-relative destination directory (""
 *  = root). The Finder calls this once per selected file. Throws on a
 *  rejected name / confinement violation / write fault. */
export function uploadFile(agentId: string, dir: string, file: File): Promise<UploadResult> {
  const q = `path=${encodeURIComponent(dir)}&name=${encodeURIComponent(file.name)}`
  return request(`/api/agent/${agentId}/fs/upload?${q}`, {
    method: "POST",
    body: file,
  })
}

/** Result of a folder creation (`POST /fs/mkdir`). */
export interface MkdirResult {
  created: string
}

/** Create a new folder `name` inside a realm directory (`dir`, "" = realm
 *  root). Powers the Finder's "New Folder" action (toolbar + empty-space
 *  context menu). The backend confines the parent dir, rejects a non-bare name
 *  or an already-existing entry (409), and returns the new folder's
 *  realm-relative path. */
export function createFolder(agentId: string, dir: string, name: string): Promise<MkdirResult> {
  const q = `path=${encodeURIComponent(dir)}&name=${encodeURIComponent(name)}`
  return request(`/api/agent/${agentId}/fs/mkdir?${q}`, { method: "POST" })
}

/** Result of a move (`POST /fs/move`). */
export interface MoveResult {
  moved: number
  skipped: number
}

/** Move one or more realm-relative entries into a destination directory
 *  (`dest`, "" = realm root). Powers the Finder's internal drag-and-drop. The
 *  backend confines both sides, refuses to clobber an existing entry, and
 *  refuses to move a folder into its own descendant. */
export function moveItems(
  agentId: string,
  items: string[],
  dest: string,
): Promise<MoveResult> {
  return request(`/api/agent/${agentId}/fs/move`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ items, dest }),
  })
}

/** Trigger a browser download for a file in the agent's realm. */
export async function downloadFile(agentId: string, path: string): Promise<void> {
  const res = await fetch(
    `${BASE}/api/agent/${agentId}/fs/download?path=${encodeURIComponent(path)}`,
  )
  if (!res.ok) {
    const body = await res.text().catch(() => res.statusText)
    throw new Error(`${res.status}: ${body}`)
  }
  const blob = await res.blob()
  const filename =
    res.headers
      .get("Content-Disposition")
      ?.match(/filename="?([^"]+)"?/)?.[1] ?? path.split("/").pop() ?? "download"
  const url = URL.createObjectURL(blob)
  const a = document.createElement("a")
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

/** One raw conversation message from `/api/agent/{id}/conversation`.
 *
 * `id` is the agent's stable `Message::id` — the SAME id the durable
 * `MessageCreated` oplog entry and the ephemeral stream `Token` frame's
 * `message_id` carry — so a live token buffer can be correlated with its
 * durable message. `uid` is the on-disk file id (distinct, not used for
 * stream correlation). */
export interface ConversationMsg {
  id: string
  uid: string
  role: string
  content: string
  timestamp_ms: number
  /** "text" | "tool_call" | "tool_result" (others tolerated). */
  message_type?: string
  tool_uses?: Array<{ id?: string; name?: string; input?: Record<string, unknown> }>
  tool_results?: Array<{ tool_name?: string; content?: string; is_error?: boolean }>
}

export function fetchConversation(agentId: string): Promise<ConversationMsg[]> {
  return request(`/api/agent/${agentId}/conversation`)
}

// ── Metrics (§19 observability) ───────────────────────────────────────

/** The §19 observability snapshot for one agent (GET /api/agent/{id}/metrics).
 *
 * Mirrors the backend `build_metrics` JSON: durable cost-breaker state, stream
 * health, and the view-vs-oplog rev lag — the figures that let the cockpit
 * *show* a tripped breaker or a lagging projection instead of inferring it. */
export interface AgentMetrics {
  id: string
  breaker: { tripped: boolean; spendUsd: number; budgetUsd: number }
  stream: { subscribers: number; droppedFrames: number; degraded: boolean }
  rev: { view: number; oplogHead: number | null; lag: number }
  /** Cumulative-since-boot token totals folded from `CostAggregate`. */
  tokens?: { input: number; output: number }
  phase?: string | null
  lifecycle?: string | null
}

export function fetchMetrics(agentId: string): Promise<AgentMetrics> {
  return request(`/api/agent/${agentId}/metrics`)
}

// ── Vitals (on-demand service-connectivity probes) ────────────────────

/** One service-connectivity probe result from `GET /api/agent/{id}/vitals`.
 *
 * `status` is honest: `"ok"` reachable, `"error"` present-but-failing,
 * `"unavailable"` when the backend genuinely cannot perform the check from
 * where it sits (mirrors the inspection plane's derived-state contract).
 * `category` groups the rows (orchestrator / agent / llm / service / infra,
 * plus `frontend` for the two rows the client adds itself). */
export interface Vital {
  name: string
  category: string
  status: "ok" | "error" | "unavailable"
  latencyMs?: number | null
  detail?: string
}

/** Run the agent's live service-connectivity checks on demand (the cockpit's
 *  "Check Vitals" button). The backend probes everything it can reach —
 *  orchestrator self, agent heartbeat + loop status, the picked LLM provider +
 *  Voyage/Datalab/Brave/Firecrawl reachability, Meilisearch, console server —
 *  and the caller prepends the two checks only the browser can observe (its own
 *  liveness + the round-trip latency of this very request). */
export function fetchVitals(agentId: string): Promise<Vital[]> {
  return request(`/api/agent/${agentId}/vitals`)
}

/** The §19 snapshot for every known agent (GET /api/metrics). Powers the fleet
 *  Usage page's live per-agent cost + token totals. */
export function fetchFleetMetrics(): Promise<AgentMetrics[]> {
  return request("/api/metrics")
}

// ── Usage + Library ───────────────────────────────────────────────────

export function fetchUsage(agentId: string): Promise<Record<string, unknown>> {
  return request(`/api/agent/${agentId}/usage`)
}

export function fetchLibrary(agentId: string): Promise<LibraryItem[]> {
  return request(`/api/agent/${agentId}/library`)
}

// ── Commands (mutating) ───────────────────────────────────────────────

export interface CommandReceipt {
  cmd_id: string
  dedup_token: string
  rev: number
  accepted: boolean
}

/** Build a full Command envelope around a Kind payload. */
function buildCommandEnvelope(kind: Record<string, unknown>): object {
  return {
    schema_version: 1,
    id: crypto.randomUUID(),
    seq: 0,
    dedup_token: crypto.randomUUID(),
    kind,
  }
}

/**
 * Send a command to an agent. Accepts just the `kind` payload —
 * the envelope (schema_version, id, seq, dedup_token) is auto-generated.
 *
 * Example: `sendCommand("agent1", { kind: "send_message", thread_id: "T1", content: "hi" })`
 */
export async function sendCommand(
  agentId: string,
  kind: Record<string, unknown>,
): Promise<CommandReceipt> {
  return request(`/api/agent/${agentId}/command`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(buildCommandEnvelope(kind)),
  })
}

// ── Ticket (for SSE) ──────────────────────────────────────────────────

export async function mintTicket(): Promise<string> {
  const res = await request<{ ticket: string }>("/api/ticket", { method: "POST" })
  return res.ticket
}
