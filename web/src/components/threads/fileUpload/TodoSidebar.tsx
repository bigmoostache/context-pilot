import { useMemo } from "react"
import { ListChecks, Circle, CircleDot, CheckCircle2 } from "lucide-react"
import type { ThreadTask } from "@/lib/types"

/**
 * Read-only right-rail sidebar listing the focused thread's todo items, sibling
 * to the Attachments {@link FileSidebar}. The task list is projected by the
 * agent (thread-owned todos, cancelled excluded) and rides the same live delta
 * plane threads do — a `task_list_changed` SSE delta folds into the thread's
 * `tasks` and re-renders this rail in real time.
 *
 * Purely presentational: the web never mutates tasks (create/mark stays
 * agent-side). Nesting (`parentId`) is rendered as indentation. Only mounted
 * when the thread has at least one task (mirrors FileSidebar's presence gate).
 */
export function TodoSidebar({ tasks }: { tasks: ThreadTask[] }) {
  // Order the flat list into a parent-before-children tree walk so nested items
  // render directly under their parent, with a depth for indentation. Roots
  // (no parentId, or a parentId whose target isn't in this thread) come first
  // in insertion order; each item's children follow it.
  const ordered = useMemo(() => orderTasks(tasks), [tasks])
  const doneCount = useMemo(() => tasks.filter((t) => t.status === "done").length, [tasks])

  return (
    <aside className="flex w-[210px] shrink-0 flex-col border-l border-border/70">
      <div className="flex items-center gap-1.5 border-b border-border/60 px-3 py-2">
        <ListChecks className="size-3 text-muted-foreground/55" />
        <span className="text-[10.5px] font-semibold tracking-wide text-muted-foreground/65 uppercase">
          Tasks
        </span>
        <span className="ml-auto rounded-full bg-muted/60 px-1.5 py-px text-[10px] font-medium text-muted-foreground/70 tabular-nums">
          {doneCount}/{tasks.length}
        </span>
      </div>
      <div className="flex-1 space-y-0.5 overflow-y-auto p-1.5">
        {ordered.map(({ task, depth }) => (
          <TaskRow key={task.id} task={task} depth={depth} />
        ))}
      </div>
    </aside>
  )
}

/** One task row: status icon + name, indented by nesting depth. */
function TaskRow({ task, depth }: { task: ThreadTask; depth: number }) {
  return (
    <div
      className="flex items-start gap-2 rounded-lg px-2 py-1.5"
      style={{ paddingLeft: `${0.5 + depth * 0.85}rem` }}
      title={task.description || undefined}
    >
      <StatusIcon status={task.status} />
      <span
        className={
          task.status === "done"
            ? "text-[11.5px] leading-tight text-muted-foreground/50 line-through"
            : "text-[11.5px] leading-tight text-foreground/85"
        }
      >
        {task.name}
      </span>
    </div>
  )
}

/** The lucide status glyph for a task, colour-keyed to the app palette. */
function StatusIcon({ status }: { status: ThreadTask["status"] }) {
  if (status === "done") {
    return <CheckCircle2 className="mt-px size-3 shrink-0 text-(--ok)" />
  }
  if (status === "in_progress") {
    return <CircleDot className="mt-px size-3 shrink-0 text-(--signal)" />
  }
  return <Circle className="mt-px size-3 shrink-0 text-muted-foreground/45" />
}

/** A task paired with its nesting depth for indentation. */
interface OrderedTask {
  task: ThreadTask
  depth: number
}

/**
 * Flatten the thread's tasks into a parent-before-children (pre-order) walk with
 * a depth per node, so children render directly beneath their parent. A task
 * whose `parentId` is absent (or points outside this thread) is treated as a
 * root. Insertion order is preserved within each sibling group.
 */
const ROOT_KEY = "\u{0}root"

function orderTasks(tasks: ThreadTask[]): OrderedTask[] {
  const byParent = new Map<string, ThreadTask[]>()
  const ids = new Set(tasks.map((t) => t.id))
  for (const t of tasks) {
    const key = t.parentId && ids.has(t.parentId) ? t.parentId : ROOT_KEY
    const bucket = byParent.get(key)
    if (bucket) bucket.push(t)
    else byParent.set(key, [t])
  }
  const out: OrderedTask[] = []
  const walk = (parentKey: string, depth: number) => {
    const children = byParent.get(parentKey) ?? []
    for (const t of children) {
      out.push({ task: t, depth })
      walk(t.id, depth + 1)
    }
  }
  walk(ROOT_KEY, 0)
  return out
}
