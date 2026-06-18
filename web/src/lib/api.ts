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

export function fetchThreads(agentId: string): Promise<ThreadDetail[]> {
  return request(`/api/agent/${agentId}/threads`)
}

// ── Panels ────────────────────────────────────────────────────────────

export function fetchPanels(agentId: string): Promise<ContextPanel[]> {
  return request(`/api/agent/${agentId}/panels`)
}

export function fetchMemory(agentId: string): Promise<MemoryCard[]> {
  return request<Record<string, Record<string, unknown>>>(`/api/agent/${agentId}/memory`).then((raw) => {
    if (Array.isArray(raw)) return raw as unknown as MemoryCard[]
    return Object.entries(raw).map(([id, m]) => ({
      id,
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
