// ── Live data hooks — TanStack Query + SSE push plane ────────────────
//
// Every hook is a thin `useQuery` wrapper over the shared `queryClient`
// (query/queryClient.ts). The hand-rolled `useLiveQuery` engine (bespoke
// setData/poll/invalidate that fought itself — T123) is gone; freshness now
// follows single-mechanism discipline (design doc §8.5):
//
//   • Delta-covered resources (threads, agent meta) ride the PUSH plane: the
//     per-agent SSE→cache bridge in `query/sync.ts` folds rev-numbered oplog
//     deltas into the cache via `setQueryData`. No refetch on a delta.
//   • Inspection resources (memory, todos, tree, …) have no oplog delta, so the
//     bridge's `invalidate` handler marks them stale → `useQuery` refetches.
//   • A slow `refetchInterval` (BACKSTOP_POLL_MS) is a last-resort safety net
//     for a dropped SSE event that reconnect-replay also missed.
//
// Hook signatures are unchanged (`LiveQueryResult<T>`) so every consumer
// component is untouched. `ensureSync(agentId)` is invoked by each agent-scoped
// hook so the push plane is guaranteed running whenever data is observed.
//
// The imperative finder/agent mutations live in ./mutations and are re-exported
// here so `@/lib/live` stays the single import surface.

import { useEffect } from "react"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import { BACKSTOP_POLL_MS } from "../query/queryClient"
import { ensureSync, mergeThreadLogs, qk } from "../query/sync"
import { getOrCreateSseClient } from "../query/sse"
import { measure, measureAsync } from "../support/telemetry"
import { useRef, useState } from "react"
import * as api from "../api"

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
} from "../types"

export * from "./mutations"

// ── Result shape (unchanged — consumers depend on this) ──────────────

export interface LiveQueryResult<T> {
  data: T | undefined
  loading: boolean
  error: Error | null
  refetch: () => void
}

/**
 * Wrap a TanStack `useQuery` into the legacy `LiveQueryResult` shape and ensure
 * the agent's SSE→cache bridge is live. `agentId` is optional: fleet-level
 * resources pass none (no bridge), agent-scoped ones pass theirs.
 */
export function useLive<T>(
  queryKey: readonly unknown[],
  queryFn: () => Promise<T>,
  opts: { agentId?: string; enabled?: boolean; pollMs?: number } = {},
): LiveQueryResult<T> {
  const { agentId, enabled = true, pollMs = BACKSTOP_POLL_MS } = opts

  // Guarantee the push plane is running for this agent whenever its data is
  // observed. Idempotent + no teardown (one long-lived subscription per agent).
  useEffect(() => {
    if (agentId) ensureSync(agentId)
  }, [agentId])

  const q = useQuery({
    queryKey,
    // Auto-attribute every polled load: time the whole fetch+parse+reshape span
    // and record it under `load:<resource>` (the queryKey head). On this
    // same-origin 127.0.0.1 cockpit the network wait is sub-ms, so a multi-
    // second span is the synchronous payload parse/reshape — the freeze suspect
    // that lands OUTSIDE the SSE fold and React render (both already
    // instrumented). A `load:*` entry coinciding with a main-thread stall names
    // exactly which endpoint's parse burned it, on every browser.
    queryFn: () => measureAsync(`load:${String(queryKey[0])}`, queryFn),
    enabled,
    refetchInterval: pollMs > 0 ? pollMs : false,
  })

  return {
    data: q.data,
    loading: q.isLoading,
    error: q.error instanceof Error ? q.error : null,
    refetch: () => {
      void q.refetch()
    },
  }
}

// ── Fleet hooks ───────────────────────────────────────────────────────

export function useFleet(): LiveQueryResult<Agent[]> {
  return useLive(qk.fleet(), () => api.fetchFleet())
}

// ── Agent-scoped hooks ────────────────────────────────────────────────

export function useAgentMeta(agentId: string): LiveQueryResult<Agent> {
  // Delta-covered: phase + cost vitals fold in via the bridge (applyAgentDelta).
  return useLive(qk.agent(agentId), () => api.fetchAgentMeta(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useThreads(agentId: string): LiveQueryResult<ThreadDetail[]> {
  const client = useQueryClient()
  // Delta-covered: roster + message deltas fold in via the bridge. The backstop
  // poll merges NON-DESTRUCTIVELY against the current cache (mergeThreadLogs) so
  // a stale disk snapshot can never drop a delta-applied message/thread (T123).
  return useLive(
    qk.threads(agentId),
    async () => {
      const next = await api.fetchThreads(agentId)
      const prev = client.getQueryData<ThreadDetail[]>(qk.threads(agentId))
      // The poll reconcile is O(threads × messages) and runs on the main thread
      // on every backstop tick — a prime periodic-freeze suspect on a huge
      // roster. Name it so the HUD attributes a stall landing here.
      return measure("threads:merge", () => mergeThreadLogs(prev, next))
    },
    { agentId, enabled: !!agentId },
  )
}

// Context-panel weights power the cockpit/HUD context-budget meter. They are
// pure tier-② inspection reads with NO oplog delta to fold (panel token sizes
// are private agent working-set state, never journaled), so the only freshness
// mechanism is the poll. The default 15s backstop made the context meter feel
// frozen (T297); a brisk poll keeps it tracking within a few seconds. The
// backend read is mtime-cached, so an unchanged config.json is cheap to re-poll.
const PANELS_POLL_MS = 4000

export function usePanels(agentId: string): LiveQueryResult<ContextPanel[]> {
  return useLive(qk.panels(agentId), () => api.fetchPanels(agentId), {
    agentId,
    enabled: !!agentId,
    pollMs: PANELS_POLL_MS,
  })
}

export function useMemory(agentId: string): LiveQueryResult<MemoryCard[]> {
  return useLive(qk.memory(agentId), () => api.fetchMemory(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useTodos(agentId: string): LiveQueryResult<TodoItem[]> {
  return useLive(qk.todos(agentId), () => api.fetchTodos(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useSpine(agentId: string): LiveQueryResult<SpineNotif[]> {
  return useLive(qk.spine(agentId), () => api.fetchSpine(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useQueue(agentId: string): LiveQueryResult<QueueAction[]> {
  return useLive(qk.queue(agentId), () => api.fetchQueue(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useScratchpad(agentId: string): LiveQueryResult<ScratchCell[]> {
  return useLive(qk.scratchpad(agentId), () => api.fetchScratchpad(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useTree(agentId: string): LiveQueryResult<TreeRow[]> {
  return useLive(qk.tree(agentId), () => api.fetchTree(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useCallbacks(agentId: string): LiveQueryResult<CallbackRow[]> {
  return useLive(qk.callbacks(agentId), () => api.fetchCallbacks(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useTools(agentId: string): LiveQueryResult<ToolGroup[]> {
  return useLive(qk.tools(agentId), () => api.fetchTools(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useRadar(agentId: string): LiveQueryResult<api.RadarData> {
  return useLive(qk.radar(agentId), () => api.fetchRadar(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

export function useEntities(agentId: string): LiveQueryResult<EntityTable[]> {
  return useLive(qk.entities(agentId), () => api.fetchEntities(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

// ── Finder hooks ──────────────────────────────────────────────────────

export function useFs(agentId: string, path: string): LiveQueryResult<FinderNode[]> {
  // `fetchFs` is typed with the generated FinderNode (epoch `modified: number`);
  // the UI FinderNode is an enriched view over the same payload (relative
  // `modified` string, optional tags/created). Cast at this single seam.
  return useLive(
    qk.fs(agentId, path),
    () => api.fetchFs(agentId, path) as unknown as Promise<FinderNode[]>,
    {
      agentId,
      enabled: !!agentId,
    },
  )
}

/**
 * The agent's tree descriptions as a `{ path: description }` map, fetched once
 * per agent for the Finder's per-node info badge. Descriptions change rarely
 * (only when the agent runs its tree-describe tool), so there is no SSE bridge
 * and the backstop poll is disabled (`pollMs: 0`) — a fresh value lands on the
 * next Finder mount / agent switch, which is plenty for a static hint.
 */
export function useFsDescriptions(agentId: string): LiveQueryResult<Record<string, string>> {
  return useLive(qk.fsDescriptions(agentId), () => api.fetchDescriptions(agentId), {
    enabled: !!agentId,
    pollMs: 0,
  })
}

/**
 * Live file-content preview for the Finder Quick Look pane. Fetches a file's
 * text via the backend preview endpoint (first 256 KiB, binary rejected with a
 * 415 → surfaced as a query error so the caller renders the no-preview state).
 *
 * `enabled` gates the fetch to text-previewable selections — folders and binary
 * files never hit the endpoint. No SSE bridge: file content is not a
 * delta-covered resource, and a Quick Look preview is a point-in-time read; the
 * backstop poll is disabled (`pollMs: 0`) since the content only matters while
 * the file is selected.
 */
export function useFsPreview(
  agentId: string,
  path: string,
  enabled: boolean,
): LiveQueryResult<api.FsPreview> {
  return useLive(qk.fsPreview(agentId, path), () => api.fetchFsPreview(agentId, path), {
    enabled: enabled && !!agentId && !!path,
    pollMs: 0,
  })
}

/**
 * Live spreadsheet preview for the Finder Quick Look pane. Fetches a
 * `csv`/`tsv`/`xlsx`/`xls`/`ods` file parsed to tables via the backend sheet
 * endpoint (bounded row/col/sheet caps; a non-spreadsheet → 415 → surfaced as a
 * query error so the caller renders the no-preview state).
 *
 * `enabled` gates the fetch to spreadsheet selections. Like {@link useFsPreview}
 * it is not a delta-covered resource and a Quick Look is a point-in-time read,
 * so there is no SSE bridge and the backstop poll is disabled (`pollMs: 0`).
 */
export function useFsSheet(
  agentId: string,
  path: string,
  enabled: boolean,
): LiveQueryResult<api.SheetData> {
  return useLive(qk.fsSheet(agentId, path), () => api.fetchSheet(agentId, path), {
    enabled: enabled && !!agentId && !!path,
    pollMs: 0,
  })
}

export function useConversation(agentId: string): LiveQueryResult<api.ConversationMsg[]> {
  return useLive(qk.conversation(agentId), () => api.fetchConversation(agentId), {
    agentId,
    enabled: !!agentId,
  })
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
    const client = getOrCreateSseClient(agentId)

    const flush = () => {
      rafRef.current = null
      if (!dirtyRef.current) return
      dirtyRef.current = false
      // The per-frame token flush spreads the whole accumulation buffer and
      // triggers a conversation re-render. On a very long streamed message this
      // is a live-updating suspect the SSE-delta/poll probes don't cover — name
      // it so a stall landing here is attributed to token streaming.
      measure("stream:flush", () => setTokens({ ...bufRef.current })) // one snapshot per frame
    }
    const schedule = () => {
      if (rafRef.current != null) return
      rafRef.current = requestAnimationFrame(flush)
    }

    // Reset buffers when the agent changes (a new realm = a new stream) and
    // schedule an empty flush — never call setState synchronously in the
    // effect body (react-hooks/set-state-in-effect); the clear lands on the
    // next animation frame alongside the same rAF-batching contract.
    bufRef.current = {}
    dirtyRef.current = true
    schedule()

    const unsub = client.subscribe("stream", (event) => {
      let frame: {
        message_id?: string
        kind?: { kind?: string; text?: string }
      }
      try {
        frame = measure("stream:parse", () => JSON.parse(event.data) as typeof frame)
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
// agent mutations). So this hook rides a brisk poll (no delta fold): a tripped
// breaker or a degraded stream surfaces within one poll interval (T121).

const METRICS_POLL_MS = 4000

export function useMetrics(agentId: string): LiveQueryResult<api.AgentMetrics> {
  return useLive(qk.metrics(agentId), () => api.fetchMetrics(agentId), {
    agentId,
    enabled: !!agentId,
    pollMs: METRICS_POLL_MS,
  })
}

/**
 * Fleet-wide §19 metrics — one snapshot per known agent (`/api/metrics`).
 * Fleet scope has no single agent to subscribe to, so this rides the metrics
 * poll only (no SSE delta). Powers the Usage page's live per-agent totals.
 */
export function useFleetMetrics(): LiveQueryResult<api.AgentMetrics[]> {
  return useLive(qk.fleetMetrics(), () => api.fetchFleetMetrics(), {
    pollMs: METRICS_POLL_MS,
  })
}

// ── Library (agent-scoped) ────────────────────────────────────────────

export function useLibrary(agentId: string): LiveQueryResult<LibraryItem[]> {
  return useLive(qk.library(agentId), () => api.fetchLibrary(agentId), {
    agentId,
    enabled: !!agentId,
  })
}

// ── Commands (imperative, not hooks) ──────────────────────────────────

export { mintTicket } from "../api"
export { downloadFile } from "../api"

/**
 * Send a command to an agent and return its receipt.
 *
 * Deliberately does **not** invalidate/refetch afterwards. Every
 * command-driven mutation is covered by the push plane — the backend journals
 * an oplog delta the instant the agent applies the command, which arrives over
 * SSE in ~14 ms and is folded in-place by the `query/sync.ts` bridge (zero
 * refetch). An immediate refetch would race that delta against the lagging
 * tier-② disk cache and clobber the just-applied message (T123). Single-
 * mechanism discipline: the push plane is the sole freshness mechanism for
 * these resources; the backstop poll remains only as a documented last resort.
 */
export async function sendCommand(
  agentId: string,
  kind: Record<string, unknown>,
): Promise<api.CommandReceipt> {
  return api.sendCommand(agentId, kind)
}
