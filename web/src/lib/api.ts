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

export function fetchPreview(agentId: string, path: string): Promise<string> {
  return request(`/api/agent/${agentId}/fs/preview?path=${encodeURIComponent(path)}`)
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

export interface ConversationMsg {
  uid: string
  role: string
  content: string
  timestamp_ms: number
}

export function fetchConversation(agentId: string): Promise<ConversationMsg[]> {
  return request(`/api/agent/${agentId}/conversation`)
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
