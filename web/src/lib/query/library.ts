// ── Library-delta fold (behaviour_changed — active behaviour agent) ──
//
// Split from ./reducers for the 500-line file budget. The library is an
// inspection resource (a disk-read list), but the `behaviour_changed` delta
// CARRIES the authoritative new active-agent id, so we fold that truth straight
// into the cached list instead of waiting on a refetch — the same discipline
// threads use for `message_created`. Folding a server-emitted delta is NOT an
// optimistic guess: the delta is the backend's own confirmation, so the flag it
// sets is ground truth. This makes the footer chip flip the instant the delta
// lands, immune to any refetch/disk timing.
//
// Returns the SAME reference when no agent item's flag changes (structural
// sharing collapses the no-op), a NEW list when a flag flips, or `null` when the
// library isn't cached yet (chip not mounted) so the caller hydrates on the next
// read.

import type { LibraryItem } from "../types"
import type { OpEntry } from "./reducers"

// The unwrapped delta discriminant (`entry.kind`) — the same shape the thread /
// agent folds receive.
type Kind = OpEntry["kind"]

/**
 * Fold a `behaviour_changed` delta into the cached library: set `active` on the
 * agent item whose id matches the delta's `agent_id`, clear it on every other
 * agent item. A `null`/absent `agent_id` (revert-to-default) clears them all, so
 * the chip falls back to its "default" label — matching the backend's
 * `library()` read, which marks nothing active when `active_agent_id` is empty.
 */
export function applyLibraryDelta(
  prev: LibraryItem[] | undefined,
  k: Kind,
): LibraryItem[] | null {
  if (!prev) return null // not loaded yet (chip unmounted) → hydrate on next read
  const activeId = k.agent_id ?? null
  const next = prev.map((it) => {
    if (it.kind !== "agent") return it
    const active = activeId !== null && it.id === activeId
    return (it.active ?? false) === active ? it : { ...it, active }
  })
  // Structural sharing: if no agent item's flag flipped, every element is the
  // SAME reference, so return `prev` to make the setQueryData a no-op. A
  // post-map identity scan (not a mutable flag) keeps the change detection
  // visible to control-flow analysis (a flag mutated inside the map closure is
  // opaque to CFA — no-unnecessary-condition would read it as always-false).
  return next.some((it, i) => it !== prev[i]) ? next : prev
}
