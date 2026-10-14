// ── task_list_changed delta fold (split from ./reducers for the 500-line cap) ─
//
// The SSE `task_list_changed` delta serializes the wire `WireTask` verbatim,
// whose nesting field is snake_case `parent_id` — NOT the camelCase `parentId`
// of the REST `/agents` reshape (agent_shape.rs `reshape_task`) that the
// generated `AgentTask` type describes. This module normalizes that duality so
// the focused agent's todo nesting survives the live delta path (without it the
// tree flattens until a REST refetch re-supplies `parentId` — the T649 bug).

import type { OpEntry } from "../api/generated/types.gen"
import type { AgentDetail, AgentTask } from "../types"

// The unwrapped delta discriminant (`entry.kind`) — same alias reducers.ts uses.
type Kind = OpEntry["kind"]

/**
 * The raw task shape carried on a `task_list_changed` SSE delta. The delta
 * serializes the wire `WireTask` verbatim (snake_case `parent_id`), while the
 * generated `AgentTask` type is the camelCase REST reshape — this mirrors the
 * message plane's raw `content`/`timestamp` vs reshaped `text`/`ts` (T411), so
 * like that path we accept BOTH spellings and normalize below.
 */
interface RawTask {
  id: string
  parentId?: string | null
  parent_id?: string | null
  name: string
  description?: string
  status: AgentTask["status"]
}

/**
 * Normalize one raw delta task into the camelCase `AgentTask` the aside
 * renders — folding the wire `parent_id` onto `parentId` so nesting survives
 * the live delta path. Uses a conditional spread for `parentId` because under
 * exactOptionalPropertyTypes an explicit `undefined` is not assignable to the
 * optional slot.
 */
function normalizeTask(t: RawTask): AgentTask {
  const parentId = t.parentId ?? t.parent_id ?? null
  return {
    id: t.id,
    name: t.name,
    description: t.description ?? "",
    status: t.status,
    ...(parentId !== null && { parentId }),
  }
}

/**
 * task_list_changed — replace the target agent's `tasks` wholesale (the delta
 * carries the agent's COMPLETE cancelled-excluded list, whole-list snapshot
 * semantics). Returns `null` when the agent is unknown (→ hydrate). Each task
 * is normalized snake→camel so the nesting (`parentId`) survives the delta path
 * — the flattening fix.
 */
export function foldTaskList(prev: AgentDetail[], k: Kind): AgentDetail[] | null {
  if (prev.every((t) => t.id !== k.agent_id)) return null // unknown agent → hydrate
  const tasks = ((k.tasks ?? []) as RawTask[]).map((t) => normalizeTask(t))
  return prev.map((t) => (t.id === k.agent_id ? { ...t, tasks } : t))
}
