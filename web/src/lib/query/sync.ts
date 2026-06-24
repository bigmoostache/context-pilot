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
// The PURE folds (the OpEntry shape + applyThreadDelta/applyAgentDelta/
// mergeThreadLogs) live in ./reducers; this module owns only the imperative
// bridge — subscription, `setQueryData`, and the async spilled-body hydrate.
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
import { fetchMessageBody } from "../api"
import { applyAgentDelta, applyThreadDelta, type OpEntry } from "./reducers"
import type { Agent, ThreadDetail } from "../types"

// Re-export the pure folds + delta type so existing `@/lib/query/sync`
// consumers keep their import surface (the reducers moved to ./reducers for the
// 500-line file budget).
export { applyThreadDelta, applyAgentDelta, mergeThreadLogs, type OpEntry } from "./reducers"

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

// ── The bridge ────────────────────────────────────────────────────────

/** Per-agent high-water rev guard — drops replay dupes on SSE reconnect
 *  (delivery is rev-ordered, so a monotonic mark is sufficient). */
const lastRev = new Map<string, number>()
/** Agents whose SSE bridge is already wired (idempotent `ensureSync`). */
const wired = new Set<string>()

/**
 * Hydrate a SPILLED (large) message body and fold it into the live thread log.
 *
 * A small message rides its `MessageCreated` delta inline (`inline_body`), so
 * the synchronous {@link applyThreadDelta} reducer appends it at once. A large
 * message instead spills to the content-addressed store: its delta carries only
 * the `head` hash and no `inline_body`, so the pure reducer can't render it —
 * it merely bumps the thread's activity. Without this, a big message never
 * entered the log via the push plane and only surfaced on the 15s backstop poll
 * (the "big messages don't appear until I refresh" bug, T357).
 *
 * Here the bridge fetches the body bytes over `/body/{head}`, then re-folds the
 * delta as if it had arrived inline — reusing {@link applyThreadDelta}'s exact
 * append path (positional `msg_{n}` id derivation + dedup), so the hydrated
 * message lands identically to an inline one and the backstop poll dedups it.
 * The body is immutable and stored before the delta is emitted (the I13
 * body-before-reference barrier), so the fetch is race-free. A fetch failure
 * falls back to invalidating the threads query so a refetch still surfaces it.
 */
async function hydrateSpilledMessage(client: QueryClient, agentId: string, entry: OpEntry): Promise<void> {
  const head = entry.kind.head
  if (!head) return
  const tk = qk.threads(agentId)
  let bodyStr: string
  try {
    bodyStr = await fetchMessageBody(agentId, head)
  } catch {
    void client.invalidateQueries({ queryKey: tk }) // hydrate failed → ground-truth refetch
    return
  }
  // Re-fold as an inline delta against the FRESHEST cache (the append's id is
  // positional, so it must read the current log length at apply time).
  const synthetic: OpEntry = { ...entry, kind: { ...entry.kind, inline_body: bodyStr } }
  const prev = client.getQueryData<ThreadDetail[]>(tk)
  const next = applyThreadDelta(prev, synthetic)
  if (next === null || next === undefined) {
    if (prev !== undefined) void client.invalidateQueries({ queryKey: tk })
  } else if (next !== prev) {
    client.setQueryData(tk, next)
  }
}

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

  // A spilled (large) message has no inline body — its delta carries only the
  // `head` hash. Kick off an async hydrate that fetches the body and folds the
  // full message in (T357). The synchronous threads fold below still runs and
  // bumps the thread's activity immediately; the bubble's text lands a moment
  // later when the hydrate resolves.
  const km = entry.kind
  if (km.kind === "message_created" && !km.inline_body && km.head) {
    void hydrateSpilledMessage(client, agentId, entry)
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
