// ── SSE client for the orchestration backend ─────────────────────────
//
// Connects to /api/stream?agent=<id>&ticket=<t>, receives rev-numbered
// oplog deltas + stream hints. Reconnects automatically with
// Last-Event-ID to resume from the last seen rev.
//
// Usage:
//   const sse = createSseClient(agentId)
//   sse.subscribe("delta", (data) => { ... })
//   sse.subscribe("stream", (data) => { ... })
//   // later:
//   sse.close()

import { mintTicket } from "../api"

const BASE = (import.meta.env["VITE_API_URL"] as string | undefined) ?? "http://localhost:7878"

export type SseEventType = "delta" | "stream" | "resync" | "invalidate" | "error"

export interface SseEvent {
  type: SseEventType
  id?: string | undefined
  data: string
}

type SseListener = (event: SseEvent) => void

/** Reconnecting SSE client for one agent's event stream. */
export interface SseClient {
  /** Register a listener for a specific event type (or "*" for all). */
  subscribe(type: SseEventType | "*", listener: SseListener): () => void
  /** Permanently close (no reconnect). */
  close(): void
  /** True if currently connected. */
  readonly connected: boolean
}

/** Shared per-agent SSE clients (singleton per agentId). */
const clients = new Map<string, SseClient>()

/**
 * Get or create an SSE client for the given agent. Callers share a
 * single EventSource per agent to avoid duplicate connections.
 */
export function getOrCreateSseClient(agentId: string): SseClient {
  const existing = clients.get(agentId)
  if (existing) return existing
  const client = createSseClient(agentId)
  clients.set(agentId, client)
  return client
}

/** Remove the client from the shared map (called on close). */
function removeClient(agentId: string) {
  clients.delete(agentId)
}

const RECONNECT_BASE_MS = 1000
const RECONNECT_MAX_MS = 30_000

function createSseClient(agentId: string): SseClient {
  const listeners = new Map<string, Set<SseListener>>()
  let es: EventSource | null = null
  let lastEventId: string | undefined
  let closed = false
  // Read `closed` through a getter at sites inside synchronous callbacks
  // (es.onerror): a direct `!closed` there is narrowed by control-flow analysis
  // to the literal initializer (its only mutation is in the deferred `close()`),
  // making the guard read as always-true. A function call is opaque to CFA.
  const isClosed = () => closed
  let reconnectMs = RECONNECT_BASE_MS

  function emit(event: SseEvent) {
    const typed = listeners.get(event.type)
    if (typed) typed.forEach((fn) => fn(event))
    const wild = listeners.get("*")
    if (wild) wild.forEach((fn) => fn(event))
  }

  async function connect() {
    if (closed) return
    try {
      const ticket = await mintTicket()
      let url = `${BASE}/api/stream?agent=${encodeURIComponent(agentId)}&ticket=${encodeURIComponent(ticket)}`
      // Resume from the last seen rev. The backend reads the `last_rev` QUERY
      // param (not `last_event_id`): we disable EventSource's native
      // auto-reconnect, so the `Last-Event-ID` *header* is never sent — the
      // query param is the only resume channel. A name mismatch here makes
      // every reconnect a cold connect that re-seeds at the oplog HEAD, silently
      // dropping any deltas emitted during the disconnect window (they then only
      // surface on the slow 15s backstop poll — the T268 5-10s delay).
      if (lastEventId) url += `&last_rev=${encodeURIComponent(lastEventId)}`
      es = new EventSource(url)

      es.addEventListener("open", () => {
        reconnectMs = RECONNECT_BASE_MS
      })

      // Named events from the backend
      for (const type of ["delta", "stream", "resync", "invalidate"] as const) {
        es.addEventListener(type, (e: MessageEvent) => {
          if (e.lastEventId) lastEventId = e.lastEventId
          emit({ type, id: e.lastEventId || undefined, data: e.data as string })
        })
      }

      es.addEventListener("error", () => {
        es?.close()
        es = null
        if (!isClosed()) scheduleReconnect()
      })
    } catch {
      if (!isClosed()) scheduleReconnect()
    }
  }

  function scheduleReconnect() {
    const jitter = Math.random() * 0.5 * reconnectMs
    setTimeout(() => void connect(), reconnectMs + jitter)
    reconnectMs = Math.min(reconnectMs * 2, RECONNECT_MAX_MS)
  }

  // Start immediately
  void connect()

  const client: SseClient = {
    subscribe(type, listener) {
      let set = listeners.get(type)
      if (!set) {
        set = new Set()
        listeners.set(type, set)
      }
      set.add(listener)
      return () => {
        set.delete(listener)
        if (set.size === 0) listeners.delete(type)
      }
    },
    close() {
      closed = true
      es?.close()
      es = null
      removeClient(agentId)
    },
    get connected() {
      return es?.readyState === EventSource.OPEN
    },
  }

  return client
}
