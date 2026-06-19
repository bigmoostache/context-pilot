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

// ── Poll reconciliation (non-destructive backstop) ───────────────────
//
// The 5 s poll backstop refetches `/threads`, whose per-thread `log` is
// sourced from the tier-② disk cache (config.json) that the agent flushes on a
// debounce. A just-sent message rides the FAST push plane (a `message_created`
// delta applied in ~14–60 ms), but the disk cache lags that flush by anywhere
// from tens of ms to seconds. If the poll *replaced* the thread list wholesale
// (plain `setData(pollResult)`), a poll firing inside that lag window would
// serve a stale log WITHOUT the message and erase the delta-applied bubble —
// the exact "appears → disappears → reappears" flicker (T123, reproduced in
// `repro_t123_flicker.py`).
//
// The fix enforces single-mechanism ownership (design doc §8.5 / X859) on the
// poll path: the push plane OWNS message + thread *presence*; the disk poll is
// a non-destructive backstop that may only converge state UPWARD — refresh
// metadata and add history it knows about, but never DROP a message (or a
// freshly-created thread) the delta plane already applied. `mergeThreadLogs`
// reconciles a poll result against the current state by id: every message the
// local state has that the poll lacks is preserved, and delta-created threads
// not yet on disk are kept. Once the disk flush lands, the poll's log already
// contains the message (deduped by id) and the two planes agree — no flicker.
function mergeThreadLogs(
  prev: ThreadDetail[] | undefined,
  next: ThreadDetail[],
): ThreadDetail[] {
  if (!prev) return next // cold load — nothing to preserve yet
  const prevById = new Map(prev.map((t) => [t.id, t]))
  const merged = next.map((t) => {
    const p = prevById.get(t.id)
    if (!p || p.log.length === 0) return t
    const haveIds = new Set(t.log.map((m) => m.id))
    const extra = p.log.filter((m) => !haveIds.has(m.id))
    // Append any locally-known (delta-applied) messages the disk snapshot is
    // still missing — they are the newest, so they belong at the tail.
    return extra.length ? { ...t, log: [...t.log, ...extra] } : t
  })
  // Keep delta-created threads the disk-sourced snapshot hasn't caught up to
  // yet (same flicker class for a just-created thread). They dedupe in on a
  // later poll once the agent flushes them.
  const nextIds = new Set(next.map((t) => t.id))
  const missing = prev.filter((t) => !nextIds.has(t.id))
  return missing.length ? [...missing, ...merged] : merged
}

// ── Freshness model (single-mechanism discipline — design doc §8.5 / X865) ─
//
// Each resource has exactly ONE owner of its freshness:
//   • Delta-covered resources (threads, agent phase/cost) ride the push plane —
//     SSE `delta` events APPLIED in-place by a reducer (`applyThreadDelta` /
//     `applyAgentDelta`), zero refetch.
//   • Inspection resources (memory, todos, tree, callbacks, entities, …) have
//     no oplog delta to fold, so they ride the backend's `invalidate` SSE event
//     (emitted by the tier-② mtime backstop) → a debounced refetch.
//   • The 5 s poll is a last-resort safety net for both.
//
// A prior `invalidateAgent` bus (an in-process pub/sub that force-refetched
// every hook on any mutation) was REMOVED here: nothing called it after the
// `sendCommand` double-invalidate was cut (T123), and a bus that refetches the
// tier-② disk cache *fought* the push-plane delta it raced. One mechanism per
// resource, no two ever fighting.

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
 * @param reconcile  Optional merge applied to every fetch result (cold load,
 *   poll backstop, debounced refetch) BEFORE it becomes state:
 *   `setData(reconcile(prev, next))`. Lets a resource make its poll
 *   NON-DESTRUCTIVE — converging state upward instead of replacing it
 *   wholesale — so a stale disk-sourced snapshot can never erase a message the
 *   push plane already applied (design doc §8.5 / T123). When omitted, a fetch
 *   replaces the data wholesale (legacy).
 */
function useLiveQuery<T>(
  key: string,
  fetcher: () => Promise<T>,
  agentId?: string,
  pollMs = DEFAULT_POLL_MS,
  enabled = true,
  applyDelta?: (prev: T | undefined, entry: OpEntry) => T | null | undefined,
  reconcile?: (prev: T | undefined, next: T) => T,
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
  const reconcileRef = useRef(reconcile)
  reconcileRef.current = reconcile

  const doFetch = useCallback(() => {
    fetcher()
      .then((result) => {
        if (mountedRef.current) {
          // Non-destructive merge when a reconciler is given (the poll/cold
          // load converges state upward), else wholesale replace (legacy).
          setData((prev) =>
            reconcileRef.current ? reconcileRef.current(prev, result) : result,
          )
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
  // (the push plane), otherwise fall back to a debounced refetch. The
  // `invalidate` event is subscribed ONLY by reducer-less (inspection) hooks,
  // for which it is the sole live freshness signal; a delta-covered hook skips
  // it (single-mechanism discipline, X859 — see below).
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
    // Single-mechanism discipline (X859): a resource WITH a delta reducer is
    // delta-covered — it owns its freshness through in-place `delta` apply
    // (plus the poll backstop + SSE reconnect replay-by-rev for gaps), so it
    // must NOT also ride the tier-② `invalidate` event: that redundant refetch
    // re-reads the disk cache and races the push-plane delta it duplicates.
    // Only inspection resources (no reducer, no oplog delta to fold) keep the
    // `invalidate` subscription as their sole live freshness signal.
    const unsubInvalidate = applyDeltaRef.current
      ? undefined
      : client.subscribe("invalidate", () => debouncedRefetch())
    return () => {
      unsubDelta()
      unsubInvalidate?.()
    }
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
  // the poll backstop reconciles NON-DESTRUCTIVELY (mergeThreadLogs) so a stale
  // disk-sourced snapshot can never drop a delta-applied message/thread (T123).
  return useLiveQuery(
    `threads:${agentId}`,
    fetcher,
    agentId,
    DEFAULT_POLL_MS,
    !!agentId,
    applyThreadDelta,
    mergeThreadLogs,
  )
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

// ── Live token streaming (§7 stream plane) ────────────────────────────
//
// The durable conversation (`useConversation`) is the authoritative record,
// but it only updates once a message is flushed to disk. While the assistant
// is *typing*, the only source of the in-progress text is the ephemeral stream
// plane: the agent tees `Token` frames → backend `StreamHub` → SSE `stream`
// event. This hook consumes that channel and exposes a live, per-message text
// buffer for the conversation view to paint in real time.
//
// **§7 mandatory contract — rAF batching.** Tokens can arrive dozens of times
// per second; calling `setState` per token would thrash React. Instead each
// token is appended to a mutable ref buffer and a single state snapshot is
// flushed once per `requestAnimationFrame`. State updates are therefore capped
// at the display refresh rate (~60fps) no matter how fast tokens stream.
//
// The returned map is keyed by `message_id` (= the agent's `Message::id`, the
// same id the durable `MessageCreated`/conversation entry carries), so the
// view can correlate a live buffer with its durable message and reconcile
// (stop overriding) once the durable text catches up.

/** Per-message accumulated streaming text, keyed by `message_id`. */
export type LiveTokens = Record<string, string>

export function useStreamingTokens(agentId: string): LiveTokens {
  const [tokens, setTokens] = useState<LiveTokens>({})
  // Accumulation buffer — mutated synchronously on every token, snapshotted
  // into React state once per animation frame (never per token).
  const bufRef = useRef<LiveTokens>({})
  const dirtyRef = useRef(false)
  const rafRef = useRef<number | null>(null)

  useEffect(() => {
    if (!agentId) return
    // Reset buffers when the agent changes (a new realm = a new stream).
    bufRef.current = {}
    dirtyRef.current = false
    setTokens({})

    const client = getOrCreateSseClient(agentId)

    const flush = () => {
      rafRef.current = null
      if (!dirtyRef.current) return
      dirtyRef.current = false
      setTokens({ ...bufRef.current }) // one snapshot per frame
    }
    const schedule = () => {
      if (rafRef.current != null) return
      rafRef.current = requestAnimationFrame(flush)
    }

    const unsub = client.subscribe("stream", (event) => {
      let frame: {
        message_id?: string
        kind?: { kind?: string; text?: string }
      }
      try {
        frame = JSON.parse(event.data)
      } catch {
        return
      }
      // Internally-tagged wire enum: Token → {"kind":"token","text":"…"}.
      if (frame.kind?.kind !== "token") return
      const id = frame.message_id
      if (!id) return
      bufRef.current[id] = (bufRef.current[id] ?? "") + (frame.kind.text ?? "")
      dirtyRef.current = true
      schedule()
    })

    return () => {
      unsub()
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current)
      rafRef.current = null
      bufRef.current = {}
      dirtyRef.current = false
    }
  }, [agentId])

  return tokens
}

// ── Metrics (§19 observability — agent-scoped) ────────────────────────
//
// Health vitals (durable cost-breaker state, stream health, view-vs-oplog rev
// lag) are NOT delta-covered — there is no oplog entry whose folding yields
// "rev lag" or "subscriber count" (they are derived backend observations, not
// agent mutations). So this hook rides a short poll (no `applyDelta` reducer):
// a tripped breaker or a degraded stream surfaces within one poll interval,
// which is ample for a health indicator. Kept brisk (METRICS_POLL_MS) so a
// breaker trip the user just caused becomes visible promptly (T121).

const METRICS_POLL_MS = 4_000

export function useMetrics(agentId: string): LiveQueryResult<api.AgentMetrics> {
  const fetcher = useCallback(() => api.fetchMetrics(agentId), [agentId])
  return useLiveQuery(`metrics:${agentId}`, fetcher, agentId, METRICS_POLL_MS, !!agentId)
}

/**
 * Fleet-wide §19 metrics — one snapshot per known agent (`/api/metrics`).
 *
 * Fleet scope has no single agent to subscribe to, so this rides the metrics
 * poll only (no SSE delta). It powers the Usage page's live per-agent cost +
 * token totals, which are derived backend observations, not agent mutations.
 */
export function useFleetMetrics(): LiveQueryResult<api.AgentMetrics[]> {
  const fetcher = useCallback(() => api.fetchFleetMetrics(), [])
  return useLiveQuery("fleet-metrics", fetcher, undefined, METRICS_POLL_MS)
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
 * Send a command to an agent and return its receipt.
 *
 * Deliberately does **not** invalidate/refetch afterwards. Every
 * command-driven mutation is now covered by the push plane — the backend
 * journals an oplog delta the instant the agent applies the command
 * (`SendMessage` → `MessageCreated` + `ThreadStatusChanged`,
 * `CreateThread`/`ArchiveThread`/`RestoreThread` → the matching roster delta),
 * which arrives over SSE in ~14 ms and is applied in-place by
 * `applyThreadDelta`/`applyAgentDelta` (zero refetch). The old
 * immediate-plus-300ms refetch pair *fought* that delta: it refetched
 * `/threads` from the tier-② disk cache before the agent's debounced flush had
 * landed, so the stale snapshot clobbered the freshly-applied delta and the
 * just-sent message visibly flickered out then back in (T123).
 *
 * Single-mechanism discipline (design doc §8.5 / X859): the push plane is the
 * sole freshness mechanism for these resources; the 5 s poll in `useLiveQuery`
 * remains only as a documented last-resort backstop for an un-covered edge.
 */
export async function sendCommand(
  agentId: string,
  kind: Record<string, unknown>,
): Promise<api.CommandReceipt> {
  return api.sendCommand(agentId, kind)
}
