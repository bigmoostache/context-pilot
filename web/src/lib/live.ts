// ── Live data hooks — fetch + SSE-invalidate + poll fallback ─────────
//
// Every hook follows the same pattern:
//   1. Initial fetch() on mount (and when key changes).
//   2. SSE delta events trigger a debounced re-fetch (200ms window).
//   3. Poll backstop every `pollMs` (default 5s) as a safety net.
//
// The generic `useLiveQuery` does the heavy lifting; specific hooks are
// thin typed wrappers that components import directly.

import { useCallback, useEffect, useRef, useState } from "react"
import { getOrCreateSseClient } from "./sse"
import * as api from "./api"

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
  EntityTable,
  ThreadDetail,
  LibraryItem,
  FinderNode,
} from "./types"

// ── Oplog delta shape (the push plane payload) ────────────────────────
//
// One rev-numbered oplog entry as carried by an SSE `delta` event. Mirrors
// cp-wire `OpEntry` — an internally-tagged `kind` discriminant plus rev. We
// only need a structural subset here (the thread-roster + message kinds);
// every other kind is acknowledged and ignored by the reducers below.

interface OpEntry {
  rev: number
  timestamp_ms?: number
  kind: OpEntryKind
}

interface OpEntryKind {
  kind: string
  thread_id?: string
  name?: string
  /** ThreadTurn — snake_case "my_turn" / "their_turn". */
  status?: string
  timestamp_ms?: number
  /** Phase — snake_case "idle" / "streaming" / "tooling" (phase_transition). */
  phase?: string
  /** Cumulative spend in USD since boot (cost_aggregate). */
  cost_usd?: number
  /** Cumulative input/output tokens since boot (cost_aggregate). */
  input_tokens?: number
  output_tokens?: number
  /** Stable message id, e.g. "T7-m3" (message_created). */
  message_id?: string
  /** Content-addressed body hash, hex (message_created). */
  head?: string
  /** UTF-8 JSON message body, inlined when small (message_created). Absent
   *  when the body spilled to the content-addressed store (hydrate by head). */
  inline_body?: string
}

/** Map a wire ThreadTurn to the web ThreadStatus (MY_TURN = agent's turn). */
function turnToStatus(turn: string | undefined): ThreadDetail["status"] {
  return turn === "my_turn" ? "MY_TURN" : "THEIR_TURN"
}

/** Compact relative-time label (mirrors api.ts formatAge for synthesized rows). */
function ago(epochMs: number): string {
  const s = Math.max(0, Math.floor((Date.now() - epochMs) / 1000))
  if (s < 5) return "just now"
  if (s < 60) return `${s}s ago`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ago`
  return `${Math.floor(h / 24)}d ago`
}

/**
 * Apply one oplog `delta` to the live thread list (Leg 2 / push plane). Returns
 * a NEW list when the delta mutates the roster, the SAME list reference when the
 * delta is irrelevant to threads (cost/phase/etc. — no refetch), or `null` when
 * it can't be applied confidently (unknown id) so the caller refetches.
 */
function applyThreadDelta(
  prev: ThreadDetail[] | undefined,
  entry: OpEntry,
): ThreadDetail[] | null {
  if (!prev) return null // not loaded yet → cold refetch
  const k = entry.kind
  switch (k.kind) {
    case "thread_created": {
      if (!k.thread_id) return prev
      if (prev.some((t) => t.id === k.thread_id)) return prev // already present (idempotent)
      const ts = k.timestamp_ms ?? entry.timestamp_ms ?? Date.now()
      const created: ThreadDetail = {
        id: k.thread_id,
        name: k.name ?? "Untitled thread",
        status: turnToStatus(k.status),
        agentId: "",
        agent: "",
        createdAt: new Date(ts).toISOString(),
        lastActivity: ago(ts),
        lastActivityMs: ts,
        unread: 0,
        archived: false,
        focused: false,
        log: [],
      }
      return [created, ...prev]
    }
    case "thread_archived":
    case "thread_restored": {
      const archived = k.kind === "thread_archived"
      if (!prev.some((t) => t.id === k.thread_id)) return null
      return prev.map((t) => (t.id === k.thread_id ? { ...t, archived } : t))
    }
    case "thread_status_changed": {
      if (!prev.some((t) => t.id === k.thread_id)) return null
      return prev.map((t) =>
        t.id === k.thread_id ? { ...t, status: turnToStatus(k.status) } : t,
      )
    }
    case "message_created": {
      const thread = prev.find((t) => t.id === k.thread_id)
      if (!thread) return null // unknown thread → cold refetch
      const ts = entry.timestamp_ms ?? Date.now()
      // A spilled (large) body has no inline payload — fall back to a refetch
      // that hydrates the full log from the view/disk (rare for chat).
      if (!k.inline_body) {
        return prev.map((t) =>
          t.id === k.thread_id ? { ...t, lastActivityMs: ts, lastActivity: ago(ts) } : t,
        )
      }
      let raw: {
        id?: string
        author?: string
        text?: string | null
        ts?: number
        question?: unknown
        fileRef?: string | null
      }
      try {
        raw = JSON.parse(k.inline_body)
      } catch {
        return null // malformed body → ground-truth refetch
      }
      const msgId = raw.id ?? k.message_id ?? `${k.thread_id}-${thread.log.length}`
      if (thread.log.some((m) => m.id === msgId)) return prev // dedup (idempotent)
      const msgTs = typeof raw.ts === "number" ? raw.ts : ts
      const appended: ThreadDetail["log"][number] = {
        id: msgId,
        author: raw.author === "user" ? "user" : "assistant",
        text: raw.text ?? undefined,
        ts: new Date(msgTs).toISOString(),
        questions: raw.question
          ? (raw.question as ThreadDetail["log"][number]["questions"])
          : undefined,
        fileRef: raw.fileRef ?? undefined,
      }
      return prev.map((t) =>
        t.id === k.thread_id
          ? {
              ...t,
              log: [...t.log, appended],
              lastActivityMs: ts,
              lastActivity: ago(ts),
            }
          : t,
      )
    }
    default:
      return prev // phase / cost / lifecycle / command_effect → irrelevant to threads
  }
}

/**
 * Apply one oplog `delta` to the live agent meta (Leg 2 / push plane — phase &
 * cost vitals). Returns a NEW `Agent` when the delta moves a live vital, the
 * SAME reference when the delta is irrelevant to agent meta (e.g. a thread
 * delta reaching this hook — no work), or `null` when it can't be applied
 * confidently so the caller refetches ground truth.
 *
 * Phase maps to the maquette `status`/`accent` the fleet dot renders:
 *   - `streaming` / `tooling` → `working` (the felt "agent is busy" state),
 *     applied instantly with the matching `ok` accent;
 *   - `idle` → refetch, because idle resolves to either `needs-you` or `idle`
 *     depending on whether any thread is `MY_TURN` — a fact the phase delta
 *     alone does not carry, but the backend's `derive_status` does. A ~few-ms
 *     refetch at the *end* of work is imperceptible, and keeps the dot correct.
 *
 * Cost is cumulative-since-boot (latest-wins), so the figure is set directly.
 */
function applyAgentDelta(prev: Agent | undefined, entry: OpEntry): Agent | null {
  if (!prev) return null // not loaded yet → cold refetch
  const k = entry.kind
  switch (k.kind) {
    case "phase_transition": {
      if (k.phase === "streaming" || k.phase === "tooling") {
        if (prev.status === "working") return prev // already working — no churn
        return { ...prev, status: "working", accent: "ok" }
      }
      // idle → needs-you vs idle is a backend decision (has_my_turn) → refetch
      return null
    }
    case "cost_aggregate": {
      if (typeof k.cost_usd !== "number") return prev
      if (prev.costUsd === k.cost_usd) return prev
      return { ...prev, costUsd: k.cost_usd }
    }
    default:
      return prev // thread / lifecycle / command_effect → irrelevant to meta
  }
}

// ── Invalidation bus ──────────────────────────────────────────────────
//
// Structural solution for real-time state propagation. When agent state
// changes (command sent, TUI mutation, SSE delta), `invalidateAgent(id)`
// fires and every live hook for that agent immediately refetches from
// the server. NOT optimistic updating — the refetch returns the server's
// ground truth from the inspection plane (tier-② files).

type InvalidateFn = () => void
const invalidators = new Map<string, Set<InvalidateFn>>()

function registerInvalidator(agentId: string, fn: InvalidateFn): () => void {
  let set = invalidators.get(agentId)
  if (!set) { set = new Set(); invalidators.set(agentId, set) }
  set.add(fn)
  return () => { set!.delete(fn); if (set!.size === 0) invalidators.delete(agentId) }
}

/**
 * Force all live queries for an agent to refetch immediately.
 * Also invalidates fleet queries (agent status/thread counts may change).
 *
 * This is the **single entry point** for state invalidation — called by:
 * - `sendCommand()` after a command is accepted (web-initiated mutations)
 * - SSE `invalidate` events from the backend (TUI-initiated mutations)
 * - Manual `refetch()` from any hook consumer
 */
export function invalidateAgent(agentId: string) {
  invalidators.get(agentId)?.forEach((fn) => fn())
  // Fleet hooks register with key "" — always invalidate them too
  // (agent status, thread counts, cost may have changed)
  if (agentId !== "") invalidators.get("")?.forEach((fn) => fn())
}

// ── Generic live query ────────────────────────────────────────────────

interface LiveQueryResult<T> {
  data: T | undefined
  loading: boolean
  error: Error | null
  refetch: () => void
}

const DEFAULT_POLL_MS = 5_000
const DEBOUNCE_MS = 200

/**
 * Generic live-data hook.
 *
 * @param key    Cache/identity key — changing it resets state and re-fetches.
 * @param fetcher  Async function returning the data.
 * @param agentId  If provided, subscribe to SSE deltas for this agent.
 * @param pollMs   Poll interval in ms (0 to disable).
 * @param enabled  When false, skip fetching/polling/SSE (default true).
 * @param applyDelta  Optional reducer that APPLIES an oplog `delta` to the
 *   current data in-place (the push plane — design doc §9 / Leg 2). It receives
 *   the previous data and the decoded `OpEntry`, and returns:
 *     - a **new value** → applied immediately (zero refetch, zero debounce);
 *     - the **same reference** (prev) → delta acknowledged but irrelevant to
 *       this resource (e.g. a cost delta reaching the threads hook) — no work;
 *     - **null/undefined** → "can't apply, fall back to a refetch" (e.g. an
 *       archive for an id we don't have yet, or data not loaded).
 *   When omitted, every `delta` falls back to a debounced refetch (legacy).
 */
function useLiveQuery<T>(
  key: string,
  fetcher: () => Promise<T>,
  agentId?: string,
  pollMs = DEFAULT_POLL_MS,
  enabled = true,
  applyDelta?: (prev: T | undefined, entry: OpEntry) => T | null | undefined,
): LiveQueryResult<T> {
  const [data, setData] = useState<T | undefined>(undefined)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<Error | null>(null)
  const mountedRef = useRef(true)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  // Highest oplog rev already applied — guards against double-applying a delta
  // on SSE reconnect-replay (delivery is rev-ordered, so a simple high-water
  // mark is sufficient; design doc §6.1 monotonic-rev guard).
  const lastRevRef = useRef(-1)
  // Keep the latest data in a ref so the SSE delta handler can read the current
  // value without re-subscribing on every data change.
  const dataRef = useRef<T | undefined>(undefined)
  dataRef.current = data
  const applyDeltaRef = useRef(applyDelta)
  applyDeltaRef.current = applyDelta

  const doFetch = useCallback(() => {
    fetcher()
      .then((result) => {
        if (mountedRef.current) {
          setData(result)
          setError(null)
          setLoading(false)
        }
      })
      .catch((err: unknown) => {
        if (mountedRef.current) {
          setError(err instanceof Error ? err : new Error(String(err)))
          setLoading(false)
        }
      })
  }, [fetcher])

  /** Debounced refetch — collapses rapid SSE bursts into one fetch. */
  const debouncedRefetch = useCallback(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      debounceRef.current = null
      doFetch()
    }, DEBOUNCE_MS)
  }, [doFetch])

  // Initial fetch + reset on key change
  useEffect(() => {
    mountedRef.current = true
    setLoading(true)
    setError(null)
    setData(undefined)
    lastRevRef.current = -1
    if (enabled) doFetch()
    return () => {
      mountedRef.current = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled])

  // SSE subscription — `delta` events APPLY in-place when a reducer is given
  // (the push plane), otherwise fall back to a debounced refetch. `invalidate`
  // events always refetch (transitional safety net until every resource is
  // fully delta-covered — design doc Phase 8.5).
  useEffect(() => {
    if (!agentId || !enabled) return
    const client = getOrCreateSseClient(agentId)
    const unsubDelta = client.subscribe("delta", (event) => {
      const reducer = applyDeltaRef.current
      if (!reducer) return debouncedRefetch()
      let entry: OpEntry
      try {
        entry = JSON.parse(event.data) as OpEntry
      } catch {
        return debouncedRefetch()
      }
      // Rev-ordered delivery → high-water guard drops only true replay dupes.
      if (typeof entry.rev === "number" && entry.rev <= lastRevRef.current) return
      const next = reducer(dataRef.current, entry)
      if (next === null || next === undefined) {
        // Reducer can't apply (e.g. unknown id, or not loaded) → ground-truth refetch.
        return debouncedRefetch()
      }
      if (typeof entry.rev === "number") lastRevRef.current = entry.rev
      // Same reference = acknowledged-but-irrelevant → no state churn.
      if (next !== dataRef.current && mountedRef.current) setData(next)
    })
    const unsubInvalidate = client.subscribe("invalidate", () => debouncedRefetch())
    return () => { unsubDelta(); unsubInvalidate() }
  }, [agentId, debouncedRefetch, enabled])

  // Invalidation bus — `invalidateAgent(id)` triggers immediate refetch
  useEffect(() => {
    if (!enabled) return
    const effectiveId = agentId ?? ""
    return registerInvalidator(effectiveId, debouncedRefetch)
  }, [agentId, debouncedRefetch, enabled])

  // Poll backstop
  useEffect(() => {
    if (pollMs <= 0 || !enabled) return
    const id = setInterval(doFetch, pollMs)
    return () => clearInterval(id)
  }, [doFetch, pollMs, enabled])

  return { data, loading, error, refetch: doFetch }
}

// ── Fleet hooks ───────────────────────────────────────────────────────

export function useFleet(): LiveQueryResult<Agent[]> {
  const fetcher = useCallback(() => api.fetchFleet(), [])
  return useLiveQuery("fleet", fetcher)
}

// ── Agent-scoped hooks ────────────────────────────────────────────────

export function useAgentMeta(agentId: string): LiveQueryResult<Agent> {
  const fetcher = useCallback(() => api.fetchAgentMeta(agentId), [agentId])
  // Push plane: apply phase + cost vitals deltas in-place (sub-50ms, zero
  // refetch); a phase→idle (status may flip to needs-you) falls back to a
  // ground-truth refetch via the reducer returning null.
  return useLiveQuery(`agent:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId, applyAgentDelta)
}

export function useThreads(agentId: string): LiveQueryResult<ThreadDetail[]> {
  const fetcher = useCallback(() => api.fetchThreads(agentId), [agentId])
  // Push plane: apply thread-roster deltas in-place (sub-50ms, zero refetch);
  // anything the reducer can't apply falls back to a refetch from the view.
  return useLiveQuery(`threads:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId, applyThreadDelta)
}

export function usePanels(agentId: string): LiveQueryResult<ContextPanel[]> {
  const fetcher = useCallback(() => api.fetchPanels(agentId), [agentId])
  return useLiveQuery(`panels:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useMemory(agentId: string): LiveQueryResult<MemoryCard[]> {
  const fetcher = useCallback(() => api.fetchMemory(agentId), [agentId])
  return useLiveQuery(`memory:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useTodos(agentId: string): LiveQueryResult<TodoItem[]> {
  const fetcher = useCallback(() => api.fetchTodos(agentId), [agentId])
  return useLiveQuery(`todos:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useSpine(agentId: string): LiveQueryResult<SpineNotif[]> {
  const fetcher = useCallback(() => api.fetchSpine(agentId), [agentId])
  return useLiveQuery(`spine:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useQueue(agentId: string): LiveQueryResult<QueueAction[]> {
  const fetcher = useCallback(() => api.fetchQueue(agentId), [agentId])
  return useLiveQuery(`queue:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useScratchpad(agentId: string): LiveQueryResult<ScratchCell[]> {
  const fetcher = useCallback(() => api.fetchScratchpad(agentId), [agentId])
  return useLiveQuery(`scratchpad:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useTree(agentId: string): LiveQueryResult<TreeRow[]> {
  const fetcher = useCallback(() => api.fetchTree(agentId), [agentId])
  return useLiveQuery(`tree:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useCallbacks(agentId: string): LiveQueryResult<CallbackRow[]> {
  const fetcher = useCallback(() => api.fetchCallbacks(agentId), [agentId])
  return useLiveQuery(`callbacks:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useTools(agentId: string): LiveQueryResult<ToolGroup[]> {
  const fetcher = useCallback(() => api.fetchTools(agentId), [agentId])
  return useLiveQuery(`tools:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useRadar(agentId: string): LiveQueryResult<api.RadarData> {
  const fetcher = useCallback(() => api.fetchRadar(agentId), [agentId])
  return useLiveQuery(`radar:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useEntities(agentId: string): LiveQueryResult<EntityTable[]> {
  const fetcher = useCallback(() => api.fetchEntities(agentId), [agentId])
  return useLiveQuery(`entities:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

// ── Finder hooks ──────────────────────────────────────────────────────

export function useFs(
  agentId: string,
  path: string,
): LiveQueryResult<FinderNode[]> {
  const fetcher = useCallback(
    () => api.fetchFs(agentId, path),
    [agentId, path],
  )
  return useLiveQuery(`fs:${agentId}:${path}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useConversation(agentId: string): LiveQueryResult<api.ConversationMsg[]> {
  const fetcher = useCallback(() => api.fetchConversation(agentId), [agentId])
  return useLiveQuery(`conversation:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

// ── Library (agent-scoped) ────────────────────────────────────────────

export function useLibrary(agentId: string): LiveQueryResult<LibraryItem[]> {
  const fetcher = useCallback(() => api.fetchLibrary(agentId), [agentId])
  return useLiveQuery(`library:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

// ── Commands (imperative, not hooks) ──────────────────────────────────

export { mintTicket } from "./api"
export { downloadFile } from "./api"

/**
 * Send a command to an agent, then automatically invalidate all live
 * queries for that agent so the UI reflects the change immediately.
 *
 * Two invalidation rounds: one immediate (may catch synchronously-
 * processed commands) and one after 300ms (waits for the agent's
 * PersistenceWriter 50ms debounce + disk flush + backend mtime cache).
 */
export async function sendCommand(
  agentId: string,
  kind: Record<string, unknown>,
): Promise<api.CommandReceipt> {
  const receipt = await api.sendCommand(agentId, kind)
  invalidateAgent(agentId)
  setTimeout(() => invalidateAgent(agentId), 300)
  return receipt
}
