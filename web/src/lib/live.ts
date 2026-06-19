// ── Live data hooks — TanStack Query + SSE push plane ────────────────
//
// Every hook is a thin `useQuery` wrapper over the shared `queryClient`
// (queryClient.ts). The hand-rolled `useLiveQuery` engine (bespoke
// setData/poll/invalidate that fought itself — T123) is gone; freshness now
// follows single-mechanism discipline (design doc §8.5):
//
//   • Delta-covered resources (threads, agent meta) ride the PUSH plane: the
//     per-agent SSE→cache bridge in `sync.ts` folds rev-numbered oplog deltas
//     into the cache via `setQueryData`. No refetch on a delta.
//   • Inspection resources (memory, todos, tree, …) have no oplog delta, so the
//     bridge's `invalidate` handler marks them stale → `useQuery` refetches.
//   • A slow `refetchInterval` (BACKSTOP_POLL_MS) is a last-resort safety net
//     for a dropped SSE event that reconnect-replay also missed.
//
// Hook signatures are unchanged (`LiveQueryResult<T>`) so every consumer
// component is untouched. `ensureSync(agentId)` is invoked by each agent-scoped
// hook so the push plane is guaranteed running whenever data is observed.

import { useEffect } from "react"
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { BACKSTOP_POLL_MS } from "./queryClient"
import { ensureSync, mergeThreadLogs, qk } from "./sync"
import { getOrCreateSseClient } from "./sse"
import { useRef, useState } from "react"
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

// ── Result shape (unchanged — consumers depend on this) ──────────────

interface LiveQueryResult<T> {
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
function useLive<T>(
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
    queryFn,
    enabled,
    refetchInterval: pollMs > 0 ? pollMs : false,
  })

  return {
    data: q.data,
    loading: q.isLoading,
    error: q.error instanceof Error ? q.error : q.error ? new Error(String(q.error)) : null,
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
      return mergeThreadLogs(prev, next)
    },
    { agentId, enabled: !!agentId },
  )
}

export function usePanels(agentId: string): LiveQueryResult<ContextPanel[]> {
  return useLive(qk.panels(agentId), () => api.fetchPanels(agentId), {
    agentId,
    enabled: !!agentId,
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

export function useFs(
  agentId: string,
  path: string,
): LiveQueryResult<FinderNode[]> {
  return useLive(qk.fs(agentId, path), () => api.fetchFs(agentId, path), {
    agentId,
    enabled: !!agentId,
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
      setTokens({ ...bufRef.current }) // one snapshot per frame
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
// agent mutations). So this hook rides a brisk poll (no delta fold): a tripped
// breaker or a degraded stream surfaces within one poll interval (T121).

const METRICS_POLL_MS = 4_000

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

export { mintTicket } from "./api"
export { downloadFile } from "./api"

// ── Create agent (TanStack mutation) ──────────────────────────────────
//
// Creating an agent is a one-shot POST, not a delta-covered resource, so it
// rides a `useMutation` rather than the SSE push plane. The backend spawns the
// `cp` TUI on a pty and returns a 202 "spawning" receipt; the agent then
// self-registers and the orchestrator's registry scan discovers it within a
// second or two. We therefore invalidate the fleet query immediately AND on a
// short delay so the new card appears as soon as the agent boots, without
// waiting on the slow (15s) fleet backstop poll.

/**
 * Mutation to create a new agent. On success it nudges the fleet query to
 * refetch immediately and again shortly after, so the freshly-spawned agent
 * surfaces on the dashboard the moment it self-registers (the spawn is async —
 * the receipt only confirms the launch).
 */
// ── Finder upload (TanStack mutation) ─────────────────────────────────
//
// Uploading files is a set of one-shot POSTs (one per file — the backend takes
// raw bytes + a filename, sidestepping multipart). It is not a delta-covered
// resource, so it rides a `useMutation`. On success the current directory's
// listing query is invalidated so the new files appear immediately.

/** Max upload size per file (32 MiB) — matches the backend transport's
 *  `MAX_BODY`; a larger body would be silently truncated server-side, so we
 *  reject it client-side with a clear message instead. */
export const MAX_UPLOAD_BYTES = 32 * 1024 * 1024

/**
 * Mutation to upload one or more files into a realm directory. Files are sent
 * concurrently (one POST each); any over {@link MAX_UPLOAD_BYTES} are rejected
 * before sending. On success the destination directory's `useFs` listing is
 * invalidated so the uploads surface at once.
 */
export function useUploadFiles(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: async ({ dir, files }: { dir: string; files: File[] }) => {
      const tooBig = files.filter((f) => f.size > MAX_UPLOAD_BYTES)
      if (tooBig.length > 0) {
        throw new Error(
          `${tooBig.map((f) => f.name).join(", ")} exceeds the 32 MB upload limit`,
        )
      }
      await Promise.all(files.map((f) => api.uploadFile(agentId, dir, f)))
      return { count: files.length, dir }
    },
    onSuccess: ({ dir }) => {
      void client.invalidateQueries({ queryKey: qk.fs(agentId, dir) })
    },
  })
}

/**
 * Mutation to create a new folder inside a realm directory (the Finder's
 * "New Folder" action). Not a delta-covered resource → a `useMutation`. On
 * success the destination directory's `useFs` listing is invalidated so the
 * new folder appears at once.
 */
export function useCreateFolder(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ dir, name }: { dir: string; name: string }) =>
      api.createFolder(agentId, dir, name),
    onSuccess: (_res, { dir }) => {
      void client.invalidateQueries({ queryKey: qk.fs(agentId, dir) })
    },
  })
}

/**
 * Mutation to move one or more entries into a realm directory (the Finder's
 * internal drag-and-drop). Not a delta-covered resource → a `useMutation`. On
 * success the WHOLE `fs` query family for the agent is invalidated (both the
 * source and destination listings changed) so the move is reflected at once.
 */
export function useMoveItems(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ items, dest }: { items: string[]; dest: string }) =>
      api.moveItems(agentId, items, dest),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["fs", agentId] })
    },
  })
}

/**
 * Mutation to rename one entry in place (the Finder's inline rename). Not a
 * delta-covered resource → a `useMutation`. On success the WHOLE `fs` query
 * family for the agent is invalidated so the renamed entry surfaces under its
 * new name at once (the containing directory's listing changed).
 */
export function useRenameItem(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ path, name }: { path: string; name: string }) =>
      api.renameItem(agentId, path, name),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["fs", agentId] })
    },
  })
}

/**
 * Mutation to move one or more entries to the realm trash (the Finder's
 * right-click "Move to Trash"). Not a delta-covered resource → a `useMutation`.
 * On success the WHOLE `fs` query family for the agent is invalidated so the
 * trashed entries vanish from the current listing at once (they move into a
 * hidden `.cp-trash/` the listing never shows).
 */
export function useTrashItems(agentId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ items }: { items: string[] }) => api.trashItems(agentId, items),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["fs", agentId] })
    },
  })
}

export function useCreateAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (body: { name: string; folder?: string; model?: string }) =>
      api.createAgent(body),
    onSuccess: () => {
      const refetchFleet = () => {
        void client.invalidateQueries({ queryKey: qk.fleet() })
      }
      refetchFleet()
      // The agent self-registers in ~1-2s after the pty spawn; re-poll a couple
      // of times to catch it well before the 15s backstop.
      window.setTimeout(refetchFleet, 1500)
      window.setTimeout(refetchFleet, 3500)
    },
  })
}

/**
 * Mutation to restart an agent (kill its stale process + respawn from the
 * current binary). Like {@link useCreateAgent}, the respawn is async: the agent
 * re-registers under the same id within ~2-3s, so we nudge the fleet query to
 * refetch immediately and again shortly after, surfacing the back-to-life agent
 * well before the slow backstop poll.
 */
export function useRestartAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (agentId: string) => api.restartAgent(agentId),
    onSuccess: () => {
      const refetchFleet = () => {
        void client.invalidateQueries({ queryKey: qk.fleet() })
      }
      refetchFleet()
      window.setTimeout(refetchFleet, 2000)
      window.setTimeout(refetchFleet, 4000)
    },
  })
}

/**
 * The Retired (archived) fleet — agents stopped-but-kept (T271). Served from
 * the orchestrator's retired store (`GET /api/fleet/retired`), so each card is
 * rendered from a snapshot, not a live process. No SSE bridge (a retired agent
 * emits nothing); a slow poll keeps it eventually-consistent after a retire /
 * unretire on another tab.
 */
export function useRetiredFleet(): LiveQueryResult<Agent[]> {
  return useLive(qk.retiredFleet(), () => api.fetchRetiredFleet())
}

/**
 * Mutation to retire (archive) an agent — stop its process + console server,
 * keep its folder. On success both the active fleet and the retired fleet are
 * refetched (immediately + once more shortly after, since the process kill +
 * registry-record removal settle asynchronously) so the card moves from the
 * Active grid to the Retired section without waiting on the backstop poll.
 */
export function useRetireAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (agentId: string) => api.retireAgent(agentId),
    onSuccess: () => {
      const refetch = () => {
        void client.invalidateQueries({ queryKey: qk.fleet() })
        void client.invalidateQueries({ queryKey: qk.retiredFleet() })
      }
      refetch()
      window.setTimeout(refetch, 1500)
    },
  })
}

/**
 * Mutation to unretire an agent — clear its retired flag and respawn it on the
 * kept folder. The respawn self-registers under the same id within ~2-3s, so we
 * refetch both fleets immediately and again shortly after to surface the
 * back-to-life agent in the Active grid before the slow backstop poll.
 */
export function useUnretireAgent() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (agentId: string) => api.unretireAgent(agentId),
    onSuccess: () => {
      const refetch = () => {
        void client.invalidateQueries({ queryKey: qk.fleet() })
        void client.invalidateQueries({ queryKey: qk.retiredFleet() })
      }
      refetch()
      window.setTimeout(refetch, 2000)
      window.setTimeout(refetch, 4000)
    },
  })
}

/**
 * Send a command to an agent and return its receipt.
 *
 * Deliberately does **not** invalidate/refetch afterwards. Every
 * command-driven mutation is covered by the push plane — the backend journals
 * an oplog delta the instant the agent applies the command, which arrives over
 * SSE in ~14 ms and is folded in-place by the `sync.ts` bridge (zero refetch).
 * An immediate refetch would race that delta against the lagging tier-② disk
 * cache and clobber the just-applied message (T123). Single-mechanism
 * discipline: the push plane is the sole freshness mechanism for these
 * resources; the backstop poll remains only as a documented last resort.
 */
export async function sendCommand(
  agentId: string,
  kind: Record<string, unknown>,
): Promise<api.CommandReceipt> {
  return api.sendCommand(agentId, kind)
}
