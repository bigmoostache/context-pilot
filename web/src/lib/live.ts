// ── Live data hooks — fetch + SSE-invalidate + poll fallback ─────────
//
// Every hook follows the same pattern:
//   1. Initial fetch() on mount (and when key changes).
//   2. SSE delta events trigger a debounced re-fetch (200ms window).
//   3. Poll backstop every `pollMs` (default 5s) as a safety net.
//
// The generic `useLiveQuery` does the heavy lifting; specific hooks are
// thin typed wrappers that components import directly.

import { useCallback, useEffect, useRef, useState } from "react"
import { getOrCreateSseClient } from "./sse"
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

// ── Invalidation bus ──────────────────────────────────────────────────
//
// Structural solution for real-time state propagation. When agent state
// changes (command sent, TUI mutation, SSE delta), `invalidateAgent(id)`
// fires and every live hook for that agent immediately refetches from
// the server. NOT optimistic updating — the refetch returns the server's
// ground truth from the inspection plane (tier-② files).

type InvalidateFn = () => void
const invalidators = new Map<string, Set<InvalidateFn>>()

function registerInvalidator(agentId: string, fn: InvalidateFn): () => void {
  let set = invalidators.get(agentId)
  if (!set) { set = new Set(); invalidators.set(agentId, set) }
  set.add(fn)
  return () => { set!.delete(fn); if (set!.size === 0) invalidators.delete(agentId) }
}

/**
 * Force all live queries for an agent to refetch immediately.
 * Also invalidates fleet queries (agent status/thread counts may change).
 *
 * This is the **single entry point** for state invalidation — called by:
 * - `sendCommand()` after a command is accepted (web-initiated mutations)
 * - SSE `invalidate` events from the backend (TUI-initiated mutations)
 * - Manual `refetch()` from any hook consumer
 */
export function invalidateAgent(agentId: string) {
  invalidators.get(agentId)?.forEach((fn) => fn())
  // Fleet hooks register with key "" — always invalidate them too
  // (agent status, thread counts, cost may have changed)
  if (agentId !== "") invalidators.get("")?.forEach((fn) => fn())
}

// ── Generic live query ────────────────────────────────────────────────

interface LiveQueryResult<T> {
  data: T | undefined
  loading: boolean
  error: Error | null
  refetch: () => void
}

const DEFAULT_POLL_MS = 5_000
const DEBOUNCE_MS = 200

/**
 * Generic live-data hook.
 *
 * @param key    Cache/identity key — changing it resets state and re-fetches.
 * @param fetcher  Async function returning the data.
 * @param agentId  If provided, subscribe to SSE deltas for this agent.
 * @param pollMs   Poll interval in ms (0 to disable).
 * @param enabled  When false, skip fetching/polling/SSE (default true).
 */
function useLiveQuery<T>(
  key: string,
  fetcher: () => Promise<T>,
  agentId?: string,
  pollMs = DEFAULT_POLL_MS,
  enabled = true,
): LiveQueryResult<T> {
  const [data, setData] = useState<T | undefined>(undefined)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<Error | null>(null)
  const mountedRef = useRef(true)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const doFetch = useCallback(() => {
    fetcher()
      .then((result) => {
        if (mountedRef.current) {
          setData(result)
          setError(null)
          setLoading(false)
        }
      })
      .catch((err: unknown) => {
        if (mountedRef.current) {
          setError(err instanceof Error ? err : new Error(String(err)))
          setLoading(false)
        }
      })
  }, [fetcher])

  /** Debounced refetch — collapses rapid SSE bursts into one fetch. */
  const debouncedRefetch = useCallback(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      debounceRef.current = null
      doFetch()
    }, DEBOUNCE_MS)
  }, [doFetch])

  // Initial fetch + reset on key change
  useEffect(() => {
    mountedRef.current = true
    setLoading(true)
    setError(null)
    setData(undefined)
    if (enabled) doFetch()
    return () => {
      mountedRef.current = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled])

  // SSE subscription — delta events AND invalidate events both trigger refetch
  useEffect(() => {
    if (!agentId || !enabled) return
    const client = getOrCreateSseClient(agentId)
    const unsubDelta = client.subscribe("delta", () => debouncedRefetch())
    const unsubInvalidate = client.subscribe("invalidate", () => debouncedRefetch())
    return () => { unsubDelta(); unsubInvalidate() }
  }, [agentId, debouncedRefetch, enabled])

  // Invalidation bus — `invalidateAgent(id)` triggers immediate refetch
  useEffect(() => {
    if (!enabled) return
    const effectiveId = agentId ?? ""
    return registerInvalidator(effectiveId, debouncedRefetch)
  }, [agentId, debouncedRefetch, enabled])

  // Poll backstop
  useEffect(() => {
    if (pollMs <= 0 || !enabled) return
    const id = setInterval(doFetch, pollMs)
    return () => clearInterval(id)
  }, [doFetch, pollMs, enabled])

  return { data, loading, error, refetch: doFetch }
}

// ── Fleet hooks ───────────────────────────────────────────────────────

export function useFleet(): LiveQueryResult<Agent[]> {
  const fetcher = useCallback(() => api.fetchFleet(), [])
  return useLiveQuery("fleet", fetcher)
}

// ── Agent-scoped hooks ────────────────────────────────────────────────

export function useAgentMeta(agentId: string): LiveQueryResult<Agent> {
  const fetcher = useCallback(() => api.fetchAgentMeta(agentId), [agentId])
  return useLiveQuery(`agent:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useThreads(agentId: string): LiveQueryResult<ThreadDetail[]> {
  const fetcher = useCallback(() => api.fetchThreads(agentId), [agentId])
  return useLiveQuery(`threads:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function usePanels(agentId: string): LiveQueryResult<ContextPanel[]> {
  const fetcher = useCallback(() => api.fetchPanels(agentId), [agentId])
  return useLiveQuery(`panels:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useMemory(agentId: string): LiveQueryResult<MemoryCard[]> {
  const fetcher = useCallback(() => api.fetchMemory(agentId), [agentId])
  return useLiveQuery(`memory:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useTodos(agentId: string): LiveQueryResult<TodoItem[]> {
  const fetcher = useCallback(() => api.fetchTodos(agentId), [agentId])
  return useLiveQuery(`todos:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useSpine(agentId: string): LiveQueryResult<SpineNotif[]> {
  const fetcher = useCallback(() => api.fetchSpine(agentId), [agentId])
  return useLiveQuery(`spine:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useQueue(agentId: string): LiveQueryResult<QueueAction[]> {
  const fetcher = useCallback(() => api.fetchQueue(agentId), [agentId])
  return useLiveQuery(`queue:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useScratchpad(agentId: string): LiveQueryResult<ScratchCell[]> {
  const fetcher = useCallback(() => api.fetchScratchpad(agentId), [agentId])
  return useLiveQuery(`scratchpad:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useTree(agentId: string): LiveQueryResult<TreeRow[]> {
  const fetcher = useCallback(() => api.fetchTree(agentId), [agentId])
  return useLiveQuery(`tree:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useCallbacks(agentId: string): LiveQueryResult<CallbackRow[]> {
  const fetcher = useCallback(() => api.fetchCallbacks(agentId), [agentId])
  return useLiveQuery(`callbacks:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useTools(agentId: string): LiveQueryResult<ToolGroup[]> {
  const fetcher = useCallback(() => api.fetchTools(agentId), [agentId])
  return useLiveQuery(`tools:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useRadar(agentId: string): LiveQueryResult<api.RadarData> {
  const fetcher = useCallback(() => api.fetchRadar(agentId), [agentId])
  return useLiveQuery(`radar:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useEntities(agentId: string): LiveQueryResult<EntityTable[]> {
  const fetcher = useCallback(() => api.fetchEntities(agentId), [agentId])
  return useLiveQuery(`entities:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

// ── Finder hooks ──────────────────────────────────────────────────────

export function useFs(
  agentId: string,
  path: string,
): LiveQueryResult<FinderNode[]> {
  const fetcher = useCallback(
    () => api.fetchFs(agentId, path),
    [agentId, path],
  )
  return useLiveQuery(`fs:${agentId}:${path}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

export function useConversation(agentId: string): LiveQueryResult<api.ConversationMsg[]> {
  const fetcher = useCallback(() => api.fetchConversation(agentId), [agentId])
  return useLiveQuery(`conversation:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

// ── Library (agent-scoped) ────────────────────────────────────────────

export function useLibrary(agentId: string): LiveQueryResult<LibraryItem[]> {
  const fetcher = useCallback(() => api.fetchLibrary(agentId), [agentId])
  return useLiveQuery(`library:${agentId}`, fetcher, agentId, DEFAULT_POLL_MS, !!agentId)
}

// ── Commands (imperative, not hooks) ──────────────────────────────────

export { mintTicket } from "./api"

/**
 * Send a command to an agent, then automatically invalidate all live
 * queries for that agent so the UI reflects the change immediately.
 *
 * Two invalidation rounds: one immediate (may catch synchronously-
 * processed commands) and one after 300ms (waits for the agent's
 * PersistenceWriter 50ms debounce + disk flush + backend mtime cache).
 */
export async function sendCommand(
  agentId: string,
  kind: Record<string, unknown>,
): Promise<api.CommandReceipt> {
  const receipt = await api.sendCommand(agentId, kind)
  invalidateAgent(agentId)
  setTimeout(() => invalidateAgent(agentId), 300)
  return receipt
}
