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
//
// Both top-level reducers are thin dispatchers: the per-`kind` fold logic lives
// in the small, single-purpose helper functions above each, so the dispatcher's
// own branch count (cyclomatic complexity) and statement count stay within the
// P8 budgets and each concern is independently readable.

import type { Fleet, AgentDetail } from "../types"
import { foldTaskList } from "./taskReducer"
import { foldNoteList } from "./notesReducer"

// ── Oplog delta shape (the push-plane payload) ───────────────────────
//
// One rev-numbered oplog entry as carried by an SSE `delta` event. Mirrors
// cp-wire `OpEntry` — an internally-tagged `kind` discriminant plus rev.
// Types are generated from the OpenAPI spec (schemas_ext.rs) so the SSE
// protocol contract is mechanically enforced, not hand-maintained.

export type { OpEntry, OpEntryKind } from "../api/generated/types.gen"
import type { OpEntry } from "../api/generated/types.gen"

// The unwrapped delta discriminant (`entry.kind`) — every fold helper receives
// this already-narrowed union plus the outer entry (for its `timestamp_ms`).
type Kind = OpEntry["kind"]

// ── Helpers ───────────────────────────────────────────────────────────

/** Map a wire AgentTurn to the web AgentStatus (MY_TURN = agent's turn). */
function turnToStatus(turn: string | undefined): AgentDetail["status"] {
  return turn === "my_turn" ? "MY_TURN" : "THEIR_TURN"
}

/**
 * Stable content signature for a agent message — used to dedup the SAME
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
 * `AgentMessage`, so the strings are identical, while two genuinely distinct
 * messages can't share an exact-millisecond `ts` AND identical text AND author.
 */
function msgSignature(m: {
  author?: string | undefined
  ts?: string | number | undefined
  text?: string | undefined
}): string {
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

// ── Agent-delta fold helpers (one per `kind`, pure) ──────────────────
//
// Each takes the previous roster + the narrowed delta discriminant and returns
// the next roster, the SAME reference when the delta is a no-op (structural
// sharing collapses it), or `null` when the target is unknown and a hydrate is
// needed. Extracted from applyAgentDelta so the dispatcher stays within budget.

/**
 * agent_created — a known id is a no-op (idempotent); an unknown one returns
 * `null` so the bridge refetches `/agents`. The backend owns the row's place in
 * the tree (parent, depth, order); synthesizing it here put sub-agents at the
 * top as roots until the next poll.
 */
function foldAgentCreated(prev: AgentDetail[], k: Kind): AgentDetail[] | null {
  if (!k.agent_id) return prev
  return prev.some((t) => t.id === k.agent_id) ? prev : null
}

/**
 * Patch a single agent's boolean flag (archived / paused). Returns `null` when
 * the agent is unknown (→ hydrate). Shared by the archived/restored and
 * paused/resumed case pairs, which differ only in which field they set.
 */
function foldAgentFlag(
  prev: AgentDetail[],
  agentId: string | undefined,
  patch: Partial<AgentDetail>,
): AgentDetail[] | null {
  if (prev.every((t) => t.id !== agentId)) return null
  return prev.map((t) => (t.id === agentId ? { ...t, ...patch } : t))
}

/** agent_deleted — drop the agent (idempotent: absent → unchanged). */
function foldAgentDeleted(prev: AgentDetail[], k: Kind): AgentDetail[] {
  if (prev.every((t) => t.id !== k.agent_id)) return prev
  return prev.filter((t) => t.id !== k.agent_id)
}

/** message_deleted — remove the message at `message_ts` from its agent's log. */
function foldMessageDeleted(prev: AgentDetail[], k: Kind): AgentDetail[] | null {
  const agent = prev.find((t) => t.id === k.agent_id)
  if (!agent) return null // unknown agent → hydrate
  const target = k.message_ts
  if (target === undefined) return prev
  // The log's `ts` is epoch-ms (number from REST) or ISO string (from an
  // SSE-appended message). Normalise both to number for comparison.
  const filtered = agent.log.filter((m) => {
    const mTs = typeof m.ts === "number" ? m.ts : new Date(m.ts ?? "").getTime()
    return mTs !== target
  })
  if (filtered.length === agent.log.length) return prev // no match
  return prev.map((t) => (t.id === k.agent_id ? { ...t, log: filtered } : t))
}

/**
 * agent_focus_changed — set `focused` on the target agent and clear it
 * everywhere else, so the UI's focused-agent highlight tracks the agent in
 * real time instead of waiting on the disk-fed backstop poll. A no-op fold
 * (same flags) returns the SAME refs via map, which structural sharing collapses.
 */
function foldAgentFocus(prev: AgentDetail[], k: Kind): AgentDetail[] {
  return prev.map((t) => {
    const focused = t.id === k.agent_id
    return t.focused === focused ? t : { ...t, focused }
  })
}

/**
 * Parse an SSE `inline_body` into the raw AgentMessage shape, or `null` on
 * malformed JSON. The inline body carries the TUI agent's AgentMessage
 * (`content`/`timestamp`/`file_path`); the REST /agents endpoint reshapes
 * these to `text`/`ts`/`fileRef` (agent_shape.rs B8). Accept BOTH variants so
 * the delta reducer works regardless of source.
 */
interface RawMessage {
  id?: string
  author?: string
  text?: string | null
  content?: string | null
  ts?: number
  timestamp?: number
  fileRef?: string | null
  file_path?: string | null
  auto?: boolean
}

function parseInlineBody(inlineBody: string): RawMessage | null {
  try {
    return JSON.parse(inlineBody) as RawMessage
  } catch {
    return null // malformed → ground-truth hydrate
  }
}

/**
 * Build the appended log row from a parsed raw message at a known positional id.
 *
 * Uses conditional spreads for the optional fields: under
 * exactOptionalPropertyTypes an explicit `x: undefined` is NOT assignable to an
 * `x?: T` slot, so omit each optional field when its value is absent rather than
 * writing `undefined` into it.
 */
function buildLogRow(
  raw: RawMessage,
  msgId: string,
  fallbackTs: number,
): AgentDetail["log"][number] {
  const msgTs =
    typeof raw.ts === "number"
      ? raw.ts
      : typeof raw.timestamp === "number"
        ? raw.timestamp
        : fallbackTs
  const msgText = raw.text ?? raw.content ?? undefined
  const msgFileRef = raw.fileRef ?? raw.file_path ?? undefined
  return {
    id: msgId,
    author: raw.author === "user" ? "user" : "assistant",
    ts: new Date(msgTs).toISOString(),
    ...(msgText !== undefined && { text: msgText }),
    ...(msgFileRef !== undefined && { fileRef: msgFileRef }),
    ...(raw.auto !== undefined && { auto: raw.auto }),
  }
}

/**
 * message_created — append the pushed message to its agent's log (deduped),
 * bumping the agent's activity. Returns `null` when the agent is unknown or
 * the inline body is malformed (→ hydrate), the SAME roster when the message is
 * already present (dedup no-op) or is a spilled/large body (activity-only bump).
 */
function foldMessageCreated(prev: AgentDetail[], k: Kind, entry: OpEntry): AgentDetail[] | null {
  const agent = prev.find((t) => t.id === k.agent_id)
  if (!agent) return null // unknown agent → hydrate
  const ts = entry.timestamp_ms ?? Date.now()
  // A spilled (large) body has no inline payload — bump activity and let a
  // hydrate fetch the full log (rare for chat).
  if (!k.inline_body) {
    return prev.map((t) =>
      t.id === k.agent_id ? { ...t, lastActivityMs: ts, lastActivity: ago(ts) } : t,
    )
  }
  const raw = parseInlineBody(k.inline_body)
  if (!raw) return null // malformed → ground-truth hydrate
  // CRITICAL: id MUST match the disk-poll id so the backstop poll dedups this
  // message instead of rendering it twice. The backend `/agents` reshape ids
  // each message positionally as `msg_{index}` (agent_shape.rs reshape_message).
  // The delta folds the message at the end of the log, so its disk index == the
  // current log length. Deriving the id from that position — NOT from raw.id
  // (`{agent}-m{n}`) or k.message_id — keeps the two planes' ids identical, so
  // mergeAgentLogs collapses them to one.
  const appended = buildLogRow(raw, `msg_${agent.log.length}`, ts)
  // Dedup by positional id OR content signature (T360). The id guard alone
  // misses a message that already landed via the backstop poll under a DIFFERENT
  // positional index (the push plane runs ahead of disk, so the two planes can
  // index the same message differently) — the signature closes that gap so a
  // just-sent message can't be appended a second time.
  const sig = msgSignature(appended)
  if (agent.log.some((m) => m.id === appended.id || msgSignature(m) === sig)) return prev
  return prev.map((t) =>
    t.id === k.agent_id
      ? { ...t, log: [...t.log, appended], lastActivityMs: ts, lastActivity: ago(ts) }
      : t,
  )
}

// ── Reducers ──────────────────────────────────────────────────────────

/**
 * Fold one oplog delta into the live agent roster (the agents cache).
 *
 * A thin dispatcher over the delta `kind` — each case delegates to a
 * single-purpose fold helper above. Returns the next roster, the SAME reference
 * on an irrelevant/no-op delta, or `null` when the fold can't proceed
 * confidently (unknown id / malformed body) so the bridge hydrates ground truth.
 */
export function applyAgentDelta(
  prev: AgentDetail[] | undefined,
  entry: OpEntry,
): AgentDetail[] | null {
  if (!prev) return null // not loaded yet → invalidate/hydrate
  const k = entry.kind
  switch (k.kind) {
    case "agent_created": {
      return foldAgentCreated(prev, k)
    }
    case "agent_archived": {
      return foldAgentFlag(prev, k.agent_id, { archived: true })
    }
    case "agent_restored": {
      return foldAgentFlag(prev, k.agent_id, { archived: false })
    }
    case "agent_paused": {
      return foldAgentFlag(prev, k.agent_id, { paused: true })
    }
    case "agent_resumed": {
      return foldAgentFlag(prev, k.agent_id, { paused: false })
    }
    case "agent_deleted": {
      return foldAgentDeleted(prev, k)
    }
    case "message_deleted": {
      return foldMessageDeleted(prev, k)
    }
    case "agent_status_changed": {
      return foldAgentFlag(prev, k.agent_id, { status: turnToStatus(k.status) })
    }
    case "task_list_changed": {
      return foldTaskList(prev, k)
    }
    case "notes_changed": {
      return foldNoteList(prev, k)
    }
    case "agent_focus_changed": {
      return foldAgentFocus(prev, k)
    }
    case "message_created": {
      return foldMessageCreated(prev, k, entry)
    }
    default: {
      return prev // phase / cost / lifecycle → irrelevant to agents
    }
  }
}

// ── Agent-delta fold helpers (one per `kind`, pure) ───────────────────

/**
 * phase_transition — fold the EXACT phase (latest-wins) so the HUD shows
 * streaming vs tooling vs ready distinctly, and NEVER return null (no REST
 * hydrate on the most common transition, going idle). The coarse `status` the
 * fleet dot renders is kept in sync without a refetch: streaming/tooling →
 * "working"; idle drops a stale "working" back to "idle" (needs-you vs idle is a
 * agent-state decision the next /meta backstop reconciles — we never know it
 * from the phase delta alone).
 */
function foldPhase(prev: Fleet, k: Kind): Fleet {
  const phase = k.phase
  if (phase !== "idle" && phase !== "streaming" && phase !== "tooling") {
    return prev // unknown phase → ignore
  }
  let next: Fleet = prev
  if (prev.phase !== phase) next = { ...next, phase }
  if (phase === "streaming" || phase === "tooling") {
    if (next.status !== "working") next = { ...next, status: "working", accent: "ok" }
  } else if (next.status === "working") {
    next = { ...next, status: "idle", accent: "interactive" }
  }
  return next
}

/**
 * cost_aggregate — cumulative-since-boot (latest-wins): fold cost AND the token
 * counters so the HUD's token display rides the push plane, not the 15s poll
 * (T297).
 */
function foldCost(prev: Fleet, k: Kind): Fleet {
  let next: Fleet = prev
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

/**
 * context_usage — the agent's authoritative context-window occupancy
 * (latest-wins): fold used/threshold/budget so the HUD meter shows the EXACT
 * figure ratatui shows, plus the cache hit/miss split (hit + miss === used) so
 * the HUD can render `Used (hit)` / `Used (miss)` matching ratatui's green/amber
 * segments (T297).
 */
function foldContextUsage(prev: Fleet, k: Kind): Fleet {
  let next: Fleet = prev
  if (typeof k.used_tokens === "number" && prev.contextUsed !== k.used_tokens) {
    next = { ...next, contextUsed: k.used_tokens }
  }
  if (typeof k.threshold_tokens === "number" && prev.contextThreshold !== k.threshold_tokens) {
    next = { ...next, contextThreshold: k.threshold_tokens }
  }
  if (typeof k.budget_tokens === "number" && prev.contextBudget !== k.budget_tokens) {
    next = { ...next, contextBudget: k.budget_tokens }
  }
  if (typeof k.hit_tokens === "number" && prev.contextHit !== k.hit_tokens) {
    next = { ...next, contextHit: k.hit_tokens }
  }
  if (typeof k.miss_tokens === "number" && prev.contextMiss !== k.miss_tokens) {
    next = { ...next, contextMiss: k.miss_tokens }
  }
  return next
}

/**
 * lifecycle — fold the agent's process lifecycle (Running / Stopping) into the
 * live agent meta so the dashboard reacts within ~100ms of the oplog delta,
 * not on the next 2s registry scan.
 *
 * Stopping → status "disconnected" (process going down), UNLESS the current
 *   status is `"waiting"` (a controlled restart is in progress — the old
 *   agent's Stopping delta must not flash "disconnected").
 * Running  → status "idle" (fresh boot, clears both "disconnected" and the
 *   client-only "waiting" restart status).
 */
function foldLifecycle(prev: Fleet, k: Kind): Fleet {
  const state = k.state
  if (state === "stopping" || state === "stopped") {
    if (prev.status === "disconnected" || prev.status === "waiting") return prev
    return { ...prev, status: "disconnected", accent: "danger" }
  }
  if (state === "running") {
    if (["disconnected", "idle", "waiting"].includes(prev.status)) {
      return { ...prev, status: "idle", accent: "interactive" }
    }
    return prev
  }
  return prev
}

/**
 * Fold one oplog delta into the live agent meta (phase + cost + token vitals +
 * lifecycle).
 *
 * A thin dispatcher — each case delegates to a single-purpose fold helper above
 * and NEVER returns `null` for a known agent, so a phase change (notably going
 * idle when a stream ends) never costs a REST hydrate.
 */
export function applyFleetDelta(prev: Fleet | undefined, entry: OpEntry): Fleet | null {
  if (!prev) return null // not loaded yet → hydrate
  const k = entry.kind
  switch (k.kind) {
    case "phase_transition": {
      return foldPhase(prev, k)
    }
    case "cost_aggregate": {
      return foldCost(prev, k)
    }
    case "context_usage": {
      return foldContextUsage(prev, k)
    }
    case "lifecycle": {
      return foldLifecycle(prev, k)
    }
    default: {
      return prev // agent deltas → irrelevant to meta
    }
  }
}

// ── Non-destructive poll reconcile (the backstop merge, T123) ────────
//
// The backstop poll (`refetchInterval`) refetches `/agents`, whose per-agent
// `log` is sourced from the tier-② disk cache (config.json) the agent flushes
// on a debounce. A just-sent message rides the FAST push plane (a delta folded
// in ~14–60 ms), but the disk cache lags. If the poll REPLACED the cache
// wholesale, a poll firing inside that lag window would serve a stale log
// without the message and erase the delta-applied bubble — the T123 flicker.
//
// message (deduped by id) and the two planes agree. Agent membership and order
// always follow the poll. The agents queryFn calls
// this against the current cache (`getQueryData`) so the poll can never drop a
// delta-applied message.
export function mergeAgentLogs(
  prev: AgentDetail[] | undefined,
  next: AgentDetail[],
): AgentDetail[] {
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
    return extra.length > 0 ? { ...t, log: [...t.log, ...extra] } : t
  })
  // Membership and order are the poll's: the backend already serves agents
  // not yet flushed to disk and drops hard-deleted ones, so re-adding agents
  // the poll lacks only resurrected deleted agents.
  return merged
}
