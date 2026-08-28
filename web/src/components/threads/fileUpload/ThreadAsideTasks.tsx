import { useMemo, useState, type KeyboardEvent } from "react"
import { Circle, CircleDot, CheckCircle2, ChevronRight } from "lucide-react"
import type { ThreadTask } from "@/lib/types"

/**
 * The Tasks-tab body of {@link ThreadAside} (T662) — the thread's todo tree,
 * read-only, with one behaviour beyond the old flat {@link TodoSidebar}:
 *
 *  1. **Auto-collapse of completed branches** — a parent whose entire subtree is
 *     done/cancelled starts collapsed (its children hidden behind a chevron), so
 *     finished work folds away. Clicking the row toggles it. A parent with any
 *     still-open descendant stays expanded.
 *
 * The tree is projected by the agent (cancelled excluded upstream) and rides the
 * live `task_list_changed` delta, so this stays purely presentational.
 */
export function TaskList({ tasks }: { tasks: ThreadTask[] }) {
  // Parent → children buckets + the set of ids whose whole subtree is closed
  // (all descendants done/cancelled) — those parents default to collapsed.
  const model = useMemo(() => buildModel(tasks), [tasks])

  // User overrides on the default collapsed state, keyed by task id. Effective
  // collapsed = override ?? default. A parent not in the map follows its default.
  const [overrides, setOverrides] = useState<Record<string, boolean>>({})

  if (tasks.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-center text-[11px] text-muted-foreground/45">
        No tasks on this thread yet.
      </div>
    )
  }

  const toggle = (id: string, currentlyCollapsed: boolean) =>
    setOverrides((o) => ({ ...o, [id]: !currentlyCollapsed }))

  // Pre-order walk, skipping the subtree of any collapsed parent.
  const rows: { task: ThreadTask; depth: number; hasChildren: boolean; collapsed: boolean }[] = []
  const walk = (parentKey: string, depth: number) => {
    const children = model.childrenOf.get(parentKey) ?? []
    for (const t of children) {
      const hasChildren = model.childrenOf.has(t.id)
      const collapsed = hasChildren
        ? (overrides[t.id] ?? model.defaultCollapsed.has(t.id))
        : false
      rows.push({ task: t, depth, hasChildren, collapsed })
      if (hasChildren && !collapsed) walk(t.id, depth + 1)
    }
  }
  walk(ROOT_KEY, 0)

  return (
    <div className="space-y-0.5">
      {rows.map(({ task, depth, hasChildren, collapsed }) => (
        <TaskRow
          key={task.id}
          task={task}
          depth={depth}
          hasChildren={hasChildren}
          collapsed={collapsed}
          onToggle={() => toggle(task.id, collapsed)}
        />
      ))}
    </div>
  )
}

/** One task row: optional chevron (parents) + status icon + name. */
function TaskRow({
  task,
  depth,
  hasChildren,
  collapsed,
  onToggle,
}: {
  task: ThreadTask
  depth: number
  hasChildren: boolean
  collapsed: boolean
  onToggle: () => void
}) {
  // The label is a TIGHT-leading text line (`leading-none`) centered in a 16px
  // (`min-h-4`) flex box — the SAME height the status/chevron icons are centered
  // in. Centering a tight line box (whose box centre ≈ glyph optical centre)
  // against the icon well makes the check and the text share a true optical
  // centreline; a `text-…/4` line box would centre its 16px *box* but leave the
  // glyphs sitting optically low (the check then reads as "too high").
  const label = (
    <span className="flex min-h-4 min-w-0 items-center">
      <span
        className={
          task.status === "done"
            ? "text-[13.5px]/none text-muted-foreground/50 line-through"
            : "text-[13.5px]/none text-foreground/85 group-hover:text-foreground"
        }
      >
        {task.name}
      </span>
    </span>
  )

  return (
    <div
      className={
        "group flex items-start gap-1.5 rounded-lg px-2 py-1.5" +
        (hasChildren ? " cursor-pointer" : "")
      }
      style={{ paddingLeft: `${0.5 + depth * 0.85}rem` }}
      {...(hasChildren
        ? {
            role: "button" as const,
            tabIndex: 0,
            onClick: onToggle,
            onKeyDown: (e: KeyboardEvent) => {
              if (e.key !== "Enter" && e.key !== " ") return
              e.preventDefault()
              onToggle()
            },
          }
        : {})}
    >
      {hasChildren ? (
        <span className="flex h-4 shrink-0 items-center">
          <ChevronRight
            className={
              "size-3 text-muted-foreground/45 transition-transform" +
              (collapsed ? "" : " rotate-90")
            }
          />
        </span>
      ) : (
        <span className="h-4 w-3 shrink-0" />
      )}
      <span className="flex h-4 shrink-0 items-center">
        <StatusIcon status={task.status} />
      </span>
      {label}
    </div>
  )
}

/** The lucide status glyph for a task, colour-keyed to the app palette. */
function StatusIcon({ status }: { status: ThreadTask["status"] }) {
  if (status === "done") {
    return <CheckCircle2 className="size-3 shrink-0 text-(--ok)" />
  }
  if (status === "in_progress") {
    return <CircleDot className="size-3 shrink-0 text-(--signal)" />
  }
  return <Circle className="size-3 shrink-0 text-muted-foreground/45" />
}

const ROOT_KEY = "\u{0}root"

interface TaskModel {
  /** parentKey → ordered children (ROOT_KEY for top-level). */
  childrenOf: Map<string, ThreadTask[]>
  /** ids of parents whose entire subtree is done/cancelled (default-collapsed). */
  defaultCollapsed: Set<string>
}

/**
 * Build the parent→children map (insertion order preserved) and flag every
 * parent whose whole subtree is finished (done/cancelled) as default-collapsed.
 * A task whose `parentId` is absent or points outside this thread is a root.
 */
function buildModel(tasks: ThreadTask[]): TaskModel {
  const ids = new Set(tasks.map((t) => t.id))
  const childrenOf = new Map<string, ThreadTask[]>()
  for (const t of tasks) {
    const key = t.parentId && ids.has(t.parentId) ? t.parentId : ROOT_KEY
    const bucket = childrenOf.get(key)
    if (bucket) bucket.push(t)
    else childrenOf.set(key, [t])
  }

  const defaultCollapsed = new Set<string>()
  // Cancelled tasks are excluded upstream by the agent, so a finished leaf here
  // is simply `done`.
  const closed = (status: ThreadTask["status"]) => status === "done"
  const allClosed = (id: string): boolean => {
    const kids = childrenOf.get(id)
    if (!kids) return false // leaf — not a collapsible parent
    return kids.every((k) => closed(k.status) && (!childrenOf.has(k.id) || allClosed(k.id)))
  }
  for (const t of tasks) {
    if (childrenOf.has(t.id) && allClosed(t.id)) defaultCollapsed.add(t.id)
  }
  return { childrenOf, defaultCollapsed }
}
