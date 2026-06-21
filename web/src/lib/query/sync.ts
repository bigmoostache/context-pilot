// ── SSE → QueryClient delta bridge (the push plane, Leg 2 frontend) ──
//
// One bridge per agent. It subscribes to that agent's SSE stream and routes
// every event to the TanStack Query cache:
//
//   • `delta`      → parse the rev-numbered OpEntry and FOLD it into the live
//                    delta-covered caches (`['threads', id]`, `['agent', id]`)
//                    via `setQueryData(key, prev => reducer(prev, entry))`.
//   • `invalidate` → mark the agent's INSPECTION caches stale so they refetch
//                    (memory/todos/tree/… have no oplog delta to fold).
//
// **Why this kills the T123 class of bug structurally.** A single send emits a
// burst of two deltas (`message_created` + `thread_status_changed`) in one
// synchronous SSE macrotask. The old hand-rolled engine applied them with a
// value-form `setData(reducer(dataRef, …))`, so both reducers read the SAME
// stale snapshot and the second clobbered the first — the message vanished
// until the disk poll caught up. Here every fold is a `setQueryData` FUNCTIONAL
// updater: each delta folds onto the freshest committed cache value, and
// TanStack's structural sharing means an unchanged fold is a no-op. The burst
// can no longer clobber itself.
//
// Single-mechanism discipline (design doc §8.5): delta-covered resources own
// their freshness through these folds; inspection resources own theirs through
// `invalidate`. The two never touch the same cache key.

import type { QueryClient } from "@tanstack/react-query"
import { getOrCreateSseClient } from "./sse"
import { queryClient } from "./queryClient"
import type { Agent, ThreadDetail } from "../types"

// ── Oplog delta shape (the push-plane payload) ───────────────────────
//
// One rev-numbered oplog entry as carried by an SSE `delta` event. Mirrors
// cp-wire `OpEntry` — an internally-tagged `kind` discriminant plus rev. We
// only need a structural subset (the thread-roster + message + phase/cost
// kinds); every other kind is acknowledged and ignored by the reducers.

export interface OpEntry {
  rev: number
  timestamp_ms?: number
  kind: OpEntryKind
}

export interface OpEntryKind {
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

// ── Query keys (the single source of truth for cache identity) ───────
//
// Live, delta-covered keys ride the push plane; inspection keys ride
// `invalidate`. `key[1]` is always the agentId for agent-scoped resources, so
// the invalidate predicate can target one agent's inspection caches precisely.

export const qk = {
  fleet: () => ["fleet"] as const,
  fleetMetrics: () => ["fleet-metrics"] as const,
  retiredFleet: () => ["fleet-retired"] as const,
  agent: (id: string) => ["agent", id] as const,
  threads: (id: string) => ["threads", id] as const,
  panels: (id: string) => ["panels", id] as const,
  memory: (id: string) => ["memory", id] as const,
  todos: (id: string) => ["todos", id] as const,
  spine: (id: string) => ["spine", id] as const,
  queue: (id: string) => ["queue", id] as const,
  scratchpad: (id: string) => ["scratchpad", id] as const,
  tree: (id: string) => ["tree", id] as const,
  callbacks: (id: string) => ["callbacks", id] as const,
  tools: (id: string) => ["tools", id] as const,
  radar: (id: string) => ["radar", id] as const,
  entities: (id: string) => ["entities", id] as const,
  metrics: (id: string) => ["metrics", id] as const,
  library: (id: string) => ["library", id] as const,
  conversation: (id: string) => ["conversation", id] as const,
  fs: (id: string, path: string) => ["fs", id, path] as const,
  fsPreview: (id: string, path: string) => ["fs-preview", id, path] as const,
  fsSheet: (id: string, path: string) => ["fs-sheet", id, path] as const,
  fsDescriptions: (id: string) => ["fs-descriptions", id] as const,
} as const

/** The two delta-covered cache families — they must NEVER be invalidated (the
 *  push plane owns them). Everything else agent-scoped is inspection. */
const DELTA_COVERED = new Set(["threads", "agent"])

// ── Helpers ───────────────────────────────────────────────────────────

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

// ── Reducers (pure folds: prev cache + OpEntry → next cache) ─────────
//
// Each returns a NEW value when the delta mutates the resource, the SAME
// reference when the delta is irrelevant to it (so `setQueryData` is a no-op),
// or `null` when it can't be folded confidently (unknown id) — the bridge then
// invalidates that key so the next read hydrates ground truth.

/**
 * Fold one oplog delta into the live thread roster (the threads cache).
 */
export function applyThreadDelta(
  prev: ThreadDetail[] | undefined,
  entry: OpEntry,
): ThreadDetail[] | null {
  if (!prev) return null // not loaded yet → invalidate/hydrate
  const k = entry.kind
  switch (k.kind) {
    case "thread_created": {
      if (!k.thread_id) return prev
      if (prev.some((t) => t.id === k.thread_id)) return prev // idempotent
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
    case "thread_focus_changed": {
      // Focus moved to k.thread_id (or was released when undefined). Set the
      // focused flag on the matching thread and clear it everywhere else, so
      // the UI's focused-thread highlight tracks the agent in real time instead
      // of waiting on the disk-fed backstop poll. A no-op fold (same flags)
      // returns the SAME refs via map, which structural sharing collapses.
      return prev.map((t) => {
        const focused = t.id === k.thread_id
        return t.focused === focused ? t : { ...t, focused }
      })
    }
    case "message_created": {
      const thread = prev.find((t) => t.id === k.thread_id)
      if (!thread) return null // unknown thread → hydrate
      const ts = entry.timestamp_ms ?? Date.now()
      // A spilled (large) body has no inline payload — bump activity and let a
      // hydrate fetch the full log (rare for chat).
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
        auto?: boolean
      }
      try {
        raw = JSON.parse(k.inline_body)
      } catch {
        return null // malformed → ground-truth hydrate
      }
      // CRITICAL: id MUST match the disk-poll id so the backstop poll dedups
      // this message instead of rendering it twice. The backend `/threads`
      // reshape ids each message positionally as `msg_{index}` (thread_shape.rs
      // reshape_message). The delta folds the message at the end of the log, so
      // its disk index == the current log length. Deriving the id from that
      // position — NOT from raw.id (`{thread}-m{n}`) or k.message_id — keeps the
      // two planes' ids identical, so mergeThreadLogs collapses them to one.
      const msgId = `msg_${thread.log.length}`
      if (thread.log.some((m) => m.id === msgId)) return prev // dedup
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
        auto: raw.auto ?? undefined,
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
      return prev // phase / cost / lifecycle → irrelevant to threads
  }
}

/**
 * Fold one oplog delta into the live agent meta (phase + cost + token vitals).
 *
 * The EXACT phase (idle/streaming/tooling) is folded latest-wins so the HUD
 * renders the distinct phase, and the coarse `status`/`accent` the fleet dot
 * uses is kept in sync inline — **without ever returning `null`**, so a phase
 * change (notably going idle when a stream ends) never costs a REST hydrate.
 * Cost and the cumulative-since-boot token counters fold latest-wins too, so
 * the token display rides the push plane instead of the backstop poll (T297).
 */
export function applyAgentDelta(prev: Agent | undefined, entry: OpEntry): Agent | null {
  if (!prev) return null // not loaded yet → hydrate
  const k = entry.kind
  switch (k.kind) {
    case "phase_transition": {
      const phase = k.phase
      if (phase !== "idle" && phase !== "streaming" && phase !== "tooling") {
        return prev // unknown phase → ignore
      }
      // Fold the EXACT phase (latest-wins) so the HUD can show streaming vs
      // tooling vs ready distinctly — and NEVER return null (no REST hydrate on
      // the most common transition, going idle). The coarse `status` the fleet
      // dot renders is kept in sync without a refetch: streaming/tooling →
      // "working"; idle drops a stale "working" back to "idle" (needs-you vs
      // idle is a thread-state decision the next /meta backstop reconciles —
      // we never know it from the phase delta alone).
      let next: Agent = prev
      if (prev.phase !== phase) next = { ...next, phase }
      if (phase === "streaming" || phase === "tooling") {
        if (next.status !== "working") next = { ...next, status: "working", accent: "ok" }
      } else if (next.status === "working") {
        next = { ...next, status: "idle", accent: "interactive" }
      }
      return next
    }
    case "cost_aggregate": {
      // Cumulative-since-boot (latest-wins): fold cost AND the token counters so
      // the HUD's token display rides the push plane, not the 15s poll (T297).
      let next: Agent = prev
      if (typeof k.cost_usd === "number" && prev.costUsd !== k.cost_usd) {
        next = { ...next, costUsd: k.cost_usd }
      }
      if (typeof k.input_tokens === "number" && prev.inputTokens !== k.input_tokens) {
        next = { ...next, inputTokens: k.input_tokens }
      }
      if (typeof k.output_tokens === "number" && prev.outputTokens !== k.output_tokens) {
        next = { ...next, outputTokens: k.output_tokens }
      }
      return next
    }
    default:
      return prev // thread / lifecycle → irrelevant to meta
  }
}

// ── Non-destructive poll reconcile (the backstop merge, T123) ────────
//
// The backstop poll (`refetchInterval`) refetches `/threads`, whose per-thread
// `log` is sourced from the tier-② disk cache (config.json) the agent flushes
// on a debounce. A just-sent message rides the FAST push plane (a delta folded
// in ~14–60 ms), but the disk cache lags. If the poll REPLACED the cache
// wholesale, a poll firing inside that lag window would serve a stale log
// without the message and erase the delta-applied bubble — the T123 flicker.
//
// `mergeThreadLogs` converges state UPWARD: every message the local cache has
// that the poll lacks is preserved, and delta-created threads not yet on disk
// are kept. Once the disk flush lands, the poll's log already contains the
// message (deduped by id) and the two planes agree. The threads queryFn calls
// this against the current cache (`getQueryData`) so the poll can never drop a
// delta-applied message.
export function mergeThreadLogs(
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
    return extra.length ? { ...t, log: [...t.log, ...extra] } : t
  })
  const nextIds = new Set(next.map((t) => t.id))
  const missing = prev.filter((t) => !nextIds.has(t.id))
  return missing.length ? [...missing, ...merged] : merged
}

// ── The bridge ────────────────────────────────────────────────────────

/** Per-agent high-water rev guard — drops replay dupes on SSE reconnect
 *  (delivery is rev-ordered, so a monotonic mark is sufficient). */
const lastRev = new Map<string, number>()
/** Agents whose SSE bridge is already wired (idempotent `ensureSync`). */
const wired = new Set<string>()

/**
 * Apply one parsed delta to the agent's delta-covered caches. Folds into BOTH
 * the threads and agent-meta caches (each reducer ignores deltas not its own),
 * then advances the rev guard. A reducer verdict of `null` ("can't fold")
 * invalidates that key so the next read hydrates ground truth.
 */
function applyDelta(client: QueryClient, agentId: string, entry: OpEntry): void {
  if (typeof entry.rev === "number") {
    const seen = lastRev.get(agentId) ?? -1
    if (entry.rev <= seen) return // replay dupe
    lastRev.set(agentId, entry.rev)
  }

  // Threads cache fold.
  const tk = qk.threads(agentId)
  const tPrev = client.getQueryData<ThreadDetail[]>(tk)
  const tNext = applyThreadDelta(tPrev, entry)
  if (tNext === null || tNext === undefined) {
    if (tPrev !== undefined) void client.invalidateQueries({ queryKey: tk })
  } else if (tNext !== tPrev) {
    client.setQueryData(tk, tNext)
  }

  // Agent-meta cache fold.
  const ak = qk.agent(agentId)
  const aPrev = client.getQueryData<Agent>(ak)
  const aNext = applyAgentDelta(aPrev, entry)
  if (aNext === null || aNext === undefined) {
    if (aPrev !== undefined) void client.invalidateQueries({ queryKey: ak })
  } else if (aNext !== aPrev) {
    client.setQueryData(ak, aNext)
  }
}

/** Invalidate one agent's INSPECTION caches (delta-covered keys are exempt —
 *  the push plane owns those). Fleet-level keys (length 1) are untouched. */
function invalidateInspection(client: QueryClient, agentId: string): void {
  void client.invalidateQueries({
    predicate: (q) => {
      const key = q.queryKey
      return (
        Array.isArray(key) &&
        key.length >= 2 &&
        key[1] === agentId &&
        typeof key[0] === "string" &&
        !DELTA_COVERED.has(key[0])
      )
    },
  })
}

/**
 * Ensure the SSE→cache bridge is live for `agentId`. Idempotent: wires the
 * subscription exactly once per agent for the app lifetime (the SSE client is a
 * singleton per agent, and reconnect-replay + the rev guard keep the cache
 * correct across drops). A no-op for an empty id.
 *
 * Every live hook calls this so the push plane is guaranteed running whenever a
 * component observes an agent's data — no explicit teardown is needed because a
 * single long-lived subscription per visited agent is cheap and correct.
 */
export function ensureSync(agentId: string): void {
  if (!agentId || wired.has(agentId)) return
  wired.add(agentId)
  const client = getOrCreateSseClient(agentId)
  client.subscribe("delta", (event) => {
    let entry: OpEntry
    try {
      entry = JSON.parse(event.data) as OpEntry
    } catch {
      // Malformed frame → fall back to a ground-truth refresh of live caches.
      invalidateLiveCaches(agentId)
      return
    }
    applyDelta(queryClient, agentId, entry)
  })
  client.subscribe("invalidate", () => invalidateInspection(queryClient, agentId))
  client.subscribe("resync", () => {
    // Catastrophic gap (rev beyond the replay buffer): refresh everything for
    // this agent — live caches AND inspection — to re-anchor on ground truth.
    invalidateLiveCaches(agentId)
    invalidateInspection(queryClient, agentId)
  })
}

/** Refresh the two delta-covered caches from ground truth (used when a frame
 *  can't be folded or a resync is signalled). */
function invalidateLiveCaches(agentId: string): void {
  void queryClient.invalidateQueries({ queryKey: qk.threads(agentId) })
  void queryClient.invalidateQueries({ queryKey: qk.agent(agentId) })
}
