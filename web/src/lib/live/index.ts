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

import { useQueryClient } from "@tanstack/react-query"
import { useCallback, useEffect, useState } from "react"
import { mergeThreadLogs, qk } from "../query/sync"
import { getOrCreateSseClient } from "../query/sse"
import { useRestartAgent } from "./mutations"
import { measure } from "../support/telemetry"
import { useLive, type LiveQueryResult } from "./core"
import * as api from "../api"

import type { Agent, ThreadDetail, LibraryItem, FinderNode } from "../types"

export * from "./mutations"

// ── Live-query core (re-exported for the public `@/lib/live` surface) ─
//
// `useLive` + `LiveQueryResult` moved to ./core so ./mutations can import them
// without cycling back through this barrel (import-x/no-cycle). Re-exported here
// so every existing `@/lib/live` consumer keeps its import path unchanged.

export { useLive, type LiveQueryResult } from "./core"

// ── SSE connection state ──────────────────────────────────────────────

/**
 * Reactive SSE connection state for one agent. Returns `true` while the
 * EventSource is OPEN, `false` during reconnect / when the orchestrator is
 * unreachable. Subscribes to the shared per-agent SSE client's connection
 * callbacks so the component re-renders on open/error transitions.
 */
export function useSseConnected(agentId: string): boolean {
  const [connected, setConnected] = useState(true)

  useEffect(() => {
    if (!agentId) {
      setConnected(true)
      return
    }
    const client = getOrCreateSseClient(agentId)
    // Seed with current snapshot
    setConnected(client.connected)
    return client.subscribeConnection(setConnected)
  }, [agentId])

  return connected
}

// ── Restart lifecycle ─────────────────────────────────────────────────

/**
 * Full restart lifecycle: API call, spin, detect SSE drop, wait for
 * reconnect, stop. Handles both entry points: dialog restart (SSE up)
 * and footer restart (SSE already down). 30s failsafe timeout.
 *
 * State machine: waiting + !connected sets sawDrop; sawDrop + connected
 * clears both and invalidates the fleet + agent caches.
 */
export function useRestartFlow(agentId: string) {
  const mutation = useRestartAgent()
  const client = useQueryClient()
  const connected = useSseConnected(agentId)
  const [waiting, setWaiting] = useState(false)
  const [sawDrop, setSawDrop] = useState(false)

  useEffect(() => {
    if (!waiting) return
    if (!connected && !sawDrop) setSawDrop(true)
    if (sawDrop && connected) {
      setWaiting(false)
      setSawDrop(false)
      void client.invalidateQueries({ queryKey: qk.fleet() })
      void client.invalidateQueries({ queryKey: qk.agent(agentId) })
    }
  }, [waiting, connected, sawDrop, client, agentId])

  useEffect(() => {
    if (!waiting) return
    const t = setTimeout(() => {
      setWaiting(false)
      setSawDrop(false)
    }, 30_000)
    return () => clearTimeout(t)
  }, [waiting])

  const restart = useCallback(() => {
    if (!agentId || mutation.isPending || waiting) return
    mutation.mutate(agentId, {
      onSuccess: () => setWaiting(true),
    })
  }, [agentId, mutation, waiting])

  return {
    restart,
    restarting: mutation.isPending || waiting,
    error: mutation.error instanceof Error ? mutation.error.message : null,
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

// ── Metrics (§19 observability — agent-scoped) ────────────────────────
//
// Health vitals (stream health, view-vs-oplog rev lag) are NOT delta-covered —
// there is no oplog entry whose folding yields "rev lag" or "subscriber count"
// (they are derived backend observations, not agent mutations). So this hook
// rides a brisk poll (no delta fold): a degraded stream or lagging projection
// surfaces within one poll interval (T121).

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
