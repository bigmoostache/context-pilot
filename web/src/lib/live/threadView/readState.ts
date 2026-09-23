// ── Per-message user-read state (T712, frontend-only) ────────────────
//
// A purely client-side "the human has seen this message" receipt, kept in
// localStorage per browser — there is NO backend equivalent (the backend's
// `ThreadDetail.unread` is AGENT-side, a different question we stop surfacing).
// The list-row badge shows how many assistant messages a thread has that the
// user hasn't read; opening a thread marks all of its messages read.
// Split out of ./index for the 500-line file budget.

import { useCallback, useEffect } from "react"
import type { ThreadDetail } from "@/lib/types"

/** localStorage key holding the set of read LLM-message ids for one thread. */
const readKey = (agentId: string, threadId: string) => `cp-msg-read-${agentId}-${threadId}`

/** Ids of the LLM messages a user reads: assistant-authored, non-auto (auto
 *  tool-activity traces are collapsed noise, never "read"). */
function llmMessageIds(t: ThreadDetail): string[] {
  return t.log.filter((m) => m.author === "assistant" && !m.auto).map((m) => m.id)
}

/** Parse a thread's persisted read-id set from localStorage (empty on miss). */
function loadReadSet(agentId: string, threadId: string): Set<string> {
  try {
    const raw = localStorage.getItem(readKey(agentId, threadId))
    const arr: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(arr)
      ? new Set(arr.filter((x): x is string => typeof x === "string"))
      : new Set()
  } catch {
    return new Set()
  }
}

/**
 * Own the per-thread user-read receipts for a realm (T712). Returns
 * `unreadOf(thread)` — the count of assistant messages the user has NOT read —
 * for the list-row badge. Whenever the OPEN thread (or its log) changes, every
 * LLM message in it is persisted read, so the focused thread never shows a
 * count. Per-browser only (localStorage); a thread never opened shows all unread.
 */
export function useThreadReadState(agentId: string, threads: ThreadDetail[], openThreadId: string) {
  // Persist the open thread's LLM message ids as "read" — localStorage only, an
  // external-system sync with NO React state, so no cascading renders. The badge
  // reacts through the `openThreadId` / `threads` prop changes, not local state.
  useEffect(() => {
    const t = threads.find((x) => x.id === openThreadId)
    if (!t) return
    const ids = llmMessageIds(t)
    if (ids.length === 0) return
    const set = loadReadSet(agentId, t.id)
    const before = set.size
    for (const id of ids) set.add(id)
    if (set.size === before) return
    localStorage.setItem(readKey(agentId, t.id), JSON.stringify([...set]))
  }, [agentId, openThreadId, threads])

  const unreadOf = useCallback(
    (t: ThreadDetail): number => {
      // The open thread is being read right now → always zero (its ids are also
      // persisted by the effect above, so it stays read after you switch away).
      if (t.id === openThreadId) return 0
      const set = loadReadSet(agentId, t.id)
      return t.log.filter((m) => m.author === "assistant" && !m.auto && !set.has(m.id)).length
    },
    [agentId, openThreadId],
  )

  return { unreadOf }
}
