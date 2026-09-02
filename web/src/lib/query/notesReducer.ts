// ── notes_changed delta fold (split from ./reducers for the 500-line cap) ─
//
// The SSE `notes_changed` delta serializes the wire `WireNote` verbatim
// (id/title/content — no nesting, no snake↔camel duality unlike the task
// plane), so this fold is a plain whole-list replace. The twin of
// ./taskReducer's `foldTaskList`.

import type { OpEntry } from "../api/generated/types.gen"
import type { ThreadDetail, ThreadNote } from "../types"

// The unwrapped delta discriminant (`entry.kind`) — same alias reducers.ts uses.
type Kind = OpEntry["kind"]

/**
 * notes_changed — replace the target thread's `notes` wholesale (the delta
 * carries the thread's COMPLETE note list, whole-list snapshot semantics, just
 * like task_list_changed). Returns `null` when the thread is unknown (→ hydrate).
 * `WireNote` is a flat {id,title,content} value object, so no per-field
 * normalization is needed (contrast foldTaskList's snake→camel parentId fold).
 */
export function foldNoteList(prev: ThreadDetail[], k: Kind): ThreadDetail[] | null {
  if (prev.every((t) => t.id !== k.thread_id)) return null // unknown thread → hydrate
  const notes: ThreadNote[] = k.notes ?? []
  return prev.map((t) => (t.id === k.thread_id ? { ...t, notes } : t))
}
