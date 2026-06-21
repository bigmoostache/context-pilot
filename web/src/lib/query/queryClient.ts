// ── TanStack Query client — the one professional cache ───────────────
//
// Replaces the hand-rolled useLiveQuery cache engine (the bespoke
// setData/poll/invalidate machine that fought itself — see T123). One
// QueryClient owns every resource's cache; SSE deltas fold into it via
// `queryClient.setQueryData` (a functional updater with structural sharing),
// so the T123 "delta burst clobbers a stale snapshot" bug is structurally
// impossible: each delta folds onto the freshest committed cache value.
//
// Freshness model (single-mechanism discipline, design doc §8.5):
//   • Delta-covered resources (threads, agent meta) ride the push plane —
//     SSE deltas applied via `setQueryData` in `sync.ts`. No refetch on delta.
//   • Inspection resources (memory, todos, tree, …) have no oplog delta to
//     fold, so they ride the SSE `invalidate` event → `invalidateQueries`.
//   • A slow `refetchInterval` poll is a documented last-resort backstop only.

import { QueryClient } from "@tanstack/react-query"

/** Backstop poll cadence (ms). Deltas/invalidate drive freshness; this only
 *  heals drift from a dropped SSE event that reconnect-replay also missed. */
export const BACKSTOP_POLL_MS = 15_000

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // The SSE push plane is the freshness owner — never auto-refetch on
      // focus/mount/reconnect (that would race the delta plane, the exact
      // class of bug T123 was). Cache is driven by setQueryData + invalidate.
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
      refetchOnMount: false,
      // Data is considered fresh; deltas mutate it in place. The backstop
      // poll (per-hook refetchInterval) is the only time-based refresh.
      staleTime: Infinity,
      gcTime: 5 * 60_000,
      retry: 1,
    },
  },
})
