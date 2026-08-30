// ── "Has this element actually been looked at?" ──────────────────────
//
// Written for the SMS inbox, where it closes a real hole: an operator reads a
// message on screen — the whole body is right there — and the unread badge
// never clears, because the only thing that cleared it was a click the reader
// had no reason to make. Presence in the DOM is the wrong signal (a page of 25
// messages would mark them all read at once); intersection is the right one.

import { useEffect, useState } from "react"

/** How much of the element must be visible before it counts as seen. Half, so
 *  a row peeking one pixel above the fold does not count. */
const THRESHOLD = 0.5

/** Whether this engine can answer the question at all.
 *
 *  Where it cannot (jsdom, an old engine) the answer is "seen" from the start:
 *  failing to clear a badge is a worse outcome than clearing it a beat early,
 *  and the server is idempotent about being told twice. Read once, at module
 *  scope, so it seeds initial state instead of being set from inside an effect. */
const OBSERVABLE = typeof IntersectionObserver !== "undefined"

/**
 * Watch one element and report, once and for good, that it has been on screen.
 *
 * `attach` is a ref callback — pass it as `ref={attach}`. It is held in state
 * rather than a ref so the effect re-runs when the node actually mounts.
 *
 * `enabled` false means the answer is never needed (the message is already
 * read), so no observer is created at all.
 */
export function useSeen(enabled: boolean): {
  attach: (node: Element | null) => void
  seen: boolean
} {
  const [node, setNode] = useState<Element | null>(null)
  const [seen, setSeen] = useState(!OBSERVABLE)

  useEffect(() => {
    if (!enabled || seen || node === null) return
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) setSeen(true)
      },
      { threshold: THRESHOLD },
    )
    observer.observe(node)
    return () => {
      observer.disconnect()
    }
  }, [enabled, seen, node])

  return { attach: setNode, seen }
}
