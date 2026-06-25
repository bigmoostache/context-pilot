// ── Oplog delta reducers (pure folds: prev cache + OpEntry → next cache) ─
//
// Split from ./sync for the 500-line file budget. This module is the PURE half
// of the SSE→cache bridge: the rev-numbered oplog delta shape plus the folds
// that turn `prev cache + delta → next cache`. It has no I/O and no react-query
// side effects — ./sync owns the imperative bridge (subscribe, setQueryData,
// async hydrate) and imports these reducers.
//
// Each reducer returns a NEW value when the delta mutates the resource, the
// SAME reference when the delta is irrelevant to it (so `setQueryData` is a
// no-op), or `null` when it can't be folded confidently (unknown id) — the
// bridge then invalidates that key so the next read hydrates ground truth.

import type { Agent, ThreadDetail } from "../types"
import { mapRawQuestions } from "../api"

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
  /** Context-window occupancy triple (context_usage): the agent's own
   *  used/threshold/budget tokens — folded so the web meter matches ratatui. */
  used_tokens?: number
  threshold_tokens?: number
  budget_tokens?: number
  /** cache hit/miss split of used_tokens (context_usage) — folded so the HUD's
   *  `Used (hit)` / `Used (miss)` match ratatui's green/amber bar segments. */
  hit_tokens?: number
  miss_tokens?: number
  /** Stable message id, e.g. "T7-m3" (message_created). */
  message_id?: string
  /** Content-addressed body hash, hex (message_created). */
  head?: string
  /** UTF-8 JSON message body, inlined when small (message_created). Absent
   *  when the body spilled to the content-addressed store (hydrate by head). */
  inline_body?: string
}

// ── Helpers ───────────────────────────────────────────────────────────

/** Map a wire ThreadTurn to the web ThreadStatus (MY_TURN = agent's turn). */
function turnToStatus(turn: string | undefined): ThreadDetail["status"] {
  return turn === "my_turn" ? "MY_TURN" : "THEIR_TURN"
}

/**
 * Stable content signature for a thread message — used to dedup the SAME
 * logical message across the two freshness planes when their positional
 * `msg_{n}` ids drift (T360).
 *
 * The positional id only dedups when the SSE-delta log and the backstop-poll
 * (disk) log assign the message the identical index. But the push plane runs
 * AHEAD of disk by design, so during a burst the delta appends the message at a
 * higher local index than the disk poll later assigns it — the two ids differ
 * and the id-only dedup keeps BOTH, rendering the message (notably the user's
 * just-sent one) twice. A signature over `author|ts|text` collapses them
 * regardless of index: both planes derive these three from the same stored
 * `ThreadMessage`, so the strings are identical, while two genuinely distinct
 * messages can't share an exact-millisecond `ts` AND identical text AND author.
 */
function msgSignature(m: { author?: string; ts?: string; text?: string }): string {
  return `${m.author ?? ""}|${m.ts ?? ""}|${m.text ?? ""}`
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

// ── Reducers ──────────────────────────────────────────────────────────

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
        paused: false,
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
    case "thread_paused":
    case "thread_resumed": {
      const paused = k.kind === "thread_paused"
      if (!prev.some((t) => t.id === k.thread_id)) return null
      return prev.map((t) => (t.id === k.thread_id ? { ...t, paused } : t))
    }
    case "thread_deleted": {
      if (!prev.some((t) => t.id === k.thread_id)) return prev
      return prev.filter((t) => t.id !== k.thread_id)
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
        // The SSE inline_body carries the raw ThreadMessage from the TUI agent,
        // which uses `content`/`timestamp`/`file_path`. The REST /threads
        // endpoint reshapes these to `text`/`ts`/`fileRef` (thread_shape.rs B8).
        // Accept BOTH variants so the delta reducer works regardless of source.
        text?: string | null
        content?: string | null
        ts?: number
        timestamp?: number
        question?: unknown
        fileRef?: string | null
        file_path?: string | null
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
      const msgTs = typeof raw.ts === "number" ? raw.ts : (typeof raw.timestamp === "number" ? raw.timestamp : ts)
      const msgText = raw.text ?? raw.content ?? undefined
      const msgFileRef = raw.fileRef ?? raw.file_path ?? undefined
      const candidate = {
        author: raw.author === "user" ? "user" : "assistant",
        ts: new Date(msgTs).toISOString(),
        text: msgText,
      }
      // Dedup by positional id OR content signature (T360). The id guard alone
      // misses a message that already landed via the backstop poll under a
      // DIFFERENT positional index (the push plane runs ahead of disk, so the
      // two planes can index the same message differently) — the signature
      // closes that gap so a just-sent message can't be appended a second time.
      const sig = msgSignature(candidate)
      if (thread.log.some((m) => m.id === msgId || msgSignature(m) === sig)) return prev
      const appended: ThreadDetail["log"][number] = {
        id: msgId,
        author: raw.author === "user" ? "user" : "assistant",
        text: msgText,
        ts: new Date(msgTs).toISOString(),
        questions: mapRawQuestions(raw.question),
        fileRef: msgFileRef,
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
    case "context_usage": {
      // The agent's authoritative context-window occupancy (latest-wins): fold
      // used/threshold/budget so the HUD meter shows the EXACT figure ratatui
      // shows, reactively over the push plane (T297).
      let next: Agent = prev
      if (typeof k.used_tokens === "number" && prev.contextUsed !== k.used_tokens) {
        next = { ...next, contextUsed: k.used_tokens }
      }
      if (typeof k.threshold_tokens === "number" && prev.contextThreshold !== k.threshold_tokens) {
        next = { ...next, contextThreshold: k.threshold_tokens }
      }
      if (typeof k.budget_tokens === "number" && prev.contextBudget !== k.budget_tokens) {
        next = { ...next, contextBudget: k.budget_tokens }
      }
      // The cache hit/miss split (hit + miss === used) so the HUD can render
      // `Used (hit)` / `Used (miss)` matching ratatui's green/amber segments.
      if (typeof k.hit_tokens === "number" && prev.contextHit !== k.hit_tokens) {
        next = { ...next, contextHit: k.hit_tokens }
      }
      if (typeof k.miss_tokens === "number" && prev.contextMiss !== k.miss_tokens) {
        next = { ...next, contextMiss: k.miss_tokens }
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
    const haveSigs = new Set(t.log.map((m) => msgSignature(m)))
    // Keep a local (delta-folded) message only if the poll's disk log has it
    // under neither the same positional id NOR the same content signature.
    // The signature guard is the T360 fix: the push plane indexes a message
    // ahead of disk, so the same message can carry different `msg_{n}` ids in
    // the two planes — an id-only filter would then preserve BOTH and render
    // the message twice. Matching on signature collapses that drifted-id dupe.
    const extra = p.log.filter((m) => !haveIds.has(m.id) && !haveSigs.has(msgSignature(m)))
    return extra.length ? { ...t, log: [...t.log, ...extra] } : t
  })
  const nextIds = new Set(next.map((t) => t.id))
  const missing = prev.filter((t) => !nextIds.has(t.id))
  return missing.length ? [...missing, ...merged] : merged
}
