// ── Live data hooks — TanStack Query + SSE push plane ────────────────
//
// Every hook is a thin `useQuery` wrapper over the shared `queryClient`
// (query/queryClient.ts). The hand-rolled `useLiveQuery` engine (bespoke
// setData/poll/invalidate that fought itself — T123) is gone; freshness now
// follows single-mechanism discipline (design doc §8.5):
//
//   • Delta-covered resources (agents, agent meta) ride the PUSH plane: the
//     per-agent SSE→cache bridge in `query/sync.ts` folds rev-numbered oplog
//     deltas into the cache via `setQueryData`. No refetch on a delta.
//   • Inspection resources (memory, todos, tree, …) have no oplog delta, so the
//     bridge's `invalidate` handler marks them stale → `useQuery` refetches.
//   • A slow `refetchInterval` (BACKSTOP_POLL_MS) is a last-resort safety net
//     for a dropped SSE event that reconnect-replay also missed.
//
// Hook signatures are unchanged (`LiveQueryResult<T>`) so every consumer
// component is untouched. `ensureSync(fleetId)` is invoked by each agent-scoped
// hook so the push plane is guaranteed running whenever data is observed.
//
// The imperative finder/agent mutations live in ./mutations and are re-exported
// here so `@/lib/live` stays the single import surface.

import { useQueryClient } from "@tanstack/react-query"
import { useCallback, useEffect, useState } from "react"
import { mergeAgentLogs, qk } from "../query/sync"
import { getOrCreateSseClient } from "../query/sse"
import { useRestartFleet } from "./mutations"
import { measure } from "../support/telemetry"
import { useLive, type LiveQueryResult } from "./core"
import * as api from "../api"

import type { Fleet, AgentDetail, LibraryItem, FinderNode } from "../types"

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
export function useSseConnected(fleetId: string): boolean {
  const [connected, setConnected] = useState(true)
  const [seededFor, setSeededFor] = useState<string | null>(null)

  // Seed the connection state when the agent changes — a render-phase
  // adjust-state-on-prop-change (React's documented pattern), NOT an effect (a
  // synchronous setState in an effect trips react-hooks/set-state-in-effect and
  // costs an extra commit). A brand-new client is still mid-handshake
  // (es === null → connected === false); seeding from that would flash
  // "Disconnected" on every agent switch, so stay optimistic (true) until the
  // handshake settles unless the client has connected before.
  if (seededFor !== fleetId) {
    setSeededFor(fleetId)
    if (fleetId) {
      const client = getOrCreateSseClient(fleetId)
      setConnected(client.hasEverConnected ? client.connected : true)
    } else {
      setConnected(true)
    }
  }

  // The effect ONLY subscribes to open/error transitions — no synchronous
  // setState in its body (the seed above owns the initial value).
  useEffect(() => {
    if (!fleetId) return
    const client = getOrCreateSseClient(fleetId)
    return client.subscribeConnection(setConnected)
  }, [fleetId])

  return connected
}

// ── Restart lifecycle ─────────────────────────────────────────────────

/**
 * Full restart lifecycle: API call, then set agent status to `"waiting"`.
 *
 * The new agent's natural `Lifecycle::Running` oplog delta clears `"waiting"`
 * back to `"idle"` via {@link foldLifecycle} in the reducers. The 15s backstop
 * poll also replaces `"waiting"` with the ground-truth REST status. No state
 * machine, no timers — the push plane's normal lifecycle signal is the sole
 * clearance mechanism.
 *
 * `foldLifecycle` guards `"waiting"` against the old agent's `Stopping` delta
 * (which would otherwise flash `"disconnected"` during a controlled restart).
 */
export function useRestartFlow(fleetId: string) {
  const mutation = useRestartFleet()
  const client = useQueryClient()
  const { data: fleet } = useFleetMeta(fleetId)

  const restart = useCallback(() => {
    if (!fleetId || mutation.isPending || fleet?.status === "waiting") return
    mutation.mutate(fleetId, {
      onSuccess: () => {
        client.setQueryData<Fleet>(qk.agent(fleetId), (prev) => {
          if (!prev) return prev
          return { ...prev, status: "waiting" as const, accent: "interactive" as const }
        })
      },
    })
  }, [fleetId, mutation, fleet?.status, client])

  return {
    restart,
    restarting: mutation.isPending || fleet?.status === "waiting",
    error: mutation.error instanceof Error ? mutation.error.message : null,
  }
}

// ── Fleet hooks ───────────────────────────────────────────────────────

export function useFleet(): LiveQueryResult<Fleet[]> {
  return useLive(qk.fleet(), () => api.fetchFleet())
}

// ── Agent-scoped hooks ────────────────────────────────────────────────

export function useFleetMeta(fleetId: string): LiveQueryResult<Fleet> {
  // Delta-covered: phase + cost vitals fold in via the bridge (applyFleetDelta).
  return useLive(qk.agent(fleetId), () => api.fetchFleetMeta(fleetId), {
    fleetId,
    enabled: !!fleetId,
  })
}

export function useAgents(fleetId: string): LiveQueryResult<AgentDetail[]> {
  const client = useQueryClient()
  // Delta-covered: roster + message deltas fold in via the bridge. The backstop
  // poll merges NON-DESTRUCTIVELY against the current cache (mergeAgentLogs) so
  // a stale disk snapshot can never drop a delta-applied message/agent (T123).
  return useLive(
    qk.agents(fleetId),
    async () => {
      const next = await api.fetchAgents(fleetId)
      const prev = client.getQueryData<AgentDetail[]>(qk.agents(fleetId))
      // The poll reconcile is O(agents × messages) and runs on the main agent
      // on every backstop tick — a prime periodic-freeze suspect on a huge
      // roster. Name it so the HUD attributes a stall landing here.
      return measure("agents:merge", () => mergeAgentLogs(prev, next))
    },
    { fleetId, enabled: !!fleetId },
  )
}

// ── Finder hooks ──────────────────────────────────────────────────────

export function useFs(fleetId: string, path: string): LiveQueryResult<FinderNode[]> {
  // `fetchFs` is typed with the generated FinderNode (epoch `modified: number`);
  // the UI FinderNode is an enriched view over the same payload (relative
  // `modified` string, optional tags/created). Cast at this single seam.
  return useLive(
    qk.fs(fleetId, path),
    () => api.fetchFs(fleetId, path) as unknown as Promise<FinderNode[]>,
    {
      fleetId,
      enabled: !!fleetId,
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
export function useFsDescriptions(fleetId: string): LiveQueryResult<Record<string, string>> {
  return useLive(qk.fsDescriptions(fleetId), () => api.fetchDescriptions(fleetId), {
    enabled: !!fleetId,
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
  fleetId: string,
  path: string,
  enabled: boolean,
): LiveQueryResult<api.FsPreview> {
  return useLive(qk.fsPreview(fleetId, path), () => api.fetchFsPreview(fleetId, path), {
    enabled: enabled && !!fleetId && !!path,
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
  fleetId: string,
  path: string,
  enabled: boolean,
): LiveQueryResult<api.SheetData> {
  return useLive(qk.fsSheet(fleetId, path), () => api.fetchSheet(fleetId, path), {
    enabled: enabled && !!fleetId && !!path,
    pollMs: 0,
  })
}

/**
 * Live pptx preview: the deck resolved server-side to positioned shapes.
 * Point-in-time read like {@link useFsSheet} (no SSE bridge, no poll).
 */
export function useFsSlides(
  fleetId: string,
  path: string,
  enabled: boolean,
): LiveQueryResult<api.SlideDeck> {
  return useLive(qk.fsSlides(fleetId, path), () => api.fetchSlides(fleetId, path), {
    enabled: enabled && !!fleetId && !!path,
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

export function useMetrics(fleetId: string): LiveQueryResult<api.FleetMetrics> {
  return useLive(qk.metrics(fleetId), () => api.fetchMetrics(fleetId), {
    fleetId,
    enabled: !!fleetId,
    pollMs: METRICS_POLL_MS,
  })
}

/**
 * Fleet-wide §19 metrics — one snapshot per known agent (`/api/metrics`).
 * Fleet scope has no single agent to subscribe to, so this rides the metrics
 * poll only (no SSE delta). Powers the Usage page's live per-agent totals.
 */
export function useFleetMetrics(): LiveQueryResult<api.FleetMetrics[]> {
  return useLive(qk.fleetMetrics(), () => api.fetchFleetMetrics(), {
    pollMs: METRICS_POLL_MS,
  })
}

// ── Library (agent-scoped) ────────────────────────────────────────────

export function useLibrary(fleetId: string): LiveQueryResult<LibraryItem[]> {
  return useLive(qk.library(fleetId), () => api.fetchLibrary(fleetId), {
    fleetId,
    enabled: !!fleetId,
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
  fleetId: string,
  kind: Record<string, unknown>,
): Promise<api.CommandReceipt> {
  return api.sendCommand(fleetId, kind)
}
