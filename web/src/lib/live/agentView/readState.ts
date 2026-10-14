// ── Per-message user-read state (T712, frontend-only) ────────────────
//
// A purely client-side "the human has seen this message" receipt, kept in
// localStorage per browser — there is NO backend equivalent (the backend's
// `AgentDetail.unread` is AGENT-side, a different question we stop surfacing).
// The list-row badge shows how many assistant messages a agent has that the
// user hasn't read; opening a agent marks all of its messages read.
// Split out of ./index for the 500-line file budget.

import { useCallback, useEffect } from "react"
import type { AgentDetail } from "@/lib/types"

/** localStorage key holding the set of read LLM-message ids for one agent. */
const readKey = (fleetId: string, agentId: string) => `cp-msg-read-${fleetId}-${agentId}`

/** Ids of the LLM messages a user reads: assistant-authored, non-auto (auto
 *  tool-activity traces are collapsed noise, never "read"). */
function llmMessageIds(t: AgentDetail): string[] {
  return t.log.filter((m) => m.author === "assistant" && !m.auto).map((m) => m.id)
}

/** Parse a agent's persisted read-id set from localStorage (empty on miss). */
function loadReadSet(fleetId: string, agentId: string): Set<string> {
  try {
    const raw = localStorage.getItem(readKey(fleetId, agentId))
    const arr: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(arr)
      ? new Set(arr.filter((x): x is string => typeof x === "string"))
      : new Set()
  } catch {
    return new Set()
  }
}

/**
 * Own the per-agent user-read receipts for a realm (T712). Returns
 * `unreadOf(agent)` — the count of assistant messages the user has NOT read —
 * for the list-row badge. Whenever the OPEN agent (or its log) changes, every
 * LLM message in it is persisted read, so the focused agent never shows a
 * count. Per-browser only (localStorage); a agent never opened shows all unread.
 */
export function useAgentReadState(fleetId: string, agents: AgentDetail[], openAgentId: string) {
  // Persist the open agent's LLM message ids as "read" — localStorage only, an
  // external-system sync with NO React state, so no cascading renders. The badge
  // reacts through the `openAgentId` / `agents` prop changes, not local state.
  useEffect(() => {
    const t = agents.find((x) => x.id === openAgentId)
    if (!t) return
    const ids = llmMessageIds(t)
    if (ids.length === 0) return
    const set = loadReadSet(fleetId, t.id)
    const before = set.size
    for (const id of ids) set.add(id)
    if (set.size === before) return
    localStorage.setItem(readKey(fleetId, t.id), JSON.stringify([...set]))
  }, [fleetId, openAgentId, agents])

  const unreadOf = useCallback(
    (t: AgentDetail): number => {
      // The open agent is being read right now → always zero (its ids are also
      // persisted by the effect above, so it stays read after you switch away).
      if (t.id === openAgentId) return 0
      const set = loadReadSet(fleetId, t.id)
      return t.log.filter((m) => m.author === "assistant" && !m.auto && !set.has(m.id)).length
    },
    [fleetId, openAgentId],
  )

  return { unreadOf }
}
