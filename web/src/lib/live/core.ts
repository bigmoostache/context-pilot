// ── Live-query core — the shared `useLive` wrapper + result shape ────
//
// Split out of ./index (which owns the concrete per-resource hooks) and ./
// mutations (imperative writes) so BOTH can import the wrapper from a single
// leaf module without forming an import cycle: ./index re-exports these for its
// public surface, and ./mutations imports `useLive`/`LiveQueryResult` directly
// from here instead of reaching back through the ./index barrel (which would
// close a ./index → ./mutations → ./index loop that import-x/no-cycle rejects).

import { useEffect } from "react"
import { useQuery } from "@tanstack/react-query"
import { BACKSTOP_POLL_MS } from "../query/queryClient"
import { ensureSync } from "../query/sync"
import { measureAsync } from "../support/telemetry"

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
