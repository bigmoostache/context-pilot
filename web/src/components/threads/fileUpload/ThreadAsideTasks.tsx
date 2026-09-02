import { useMemo, useState, type KeyboardEvent } from "react"
import { Square, SquareCheckBig, ChevronRight, StickyNote } from "lucide-react"
import type { ThreadTask, ThreadNote } from "@/lib/types"
import { Markdown } from "@/lib/support/markdown"

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
      const collapsed = hasChildren ? (overrides[t.id] ?? model.defaultCollapsed.has(t.id)) : false
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

/** One task row: status icon + name, then a TRAILING collapse chevron for
 *  parents (leaf rows render nothing after the name — no leading spacer needed,
 *  the label's `flex-1` pushes the chevron to the row's right edge). */
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
    <span className="flex min-h-4 min-w-0 flex-1 items-center">
      <span
        className={
          task.status === "done"
            ? "text-[13.5px]/none text-muted-foreground/50"
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
      <span className="flex h-4 shrink-0 items-center">
        <StatusIcon status={task.status} />
      </span>
      {label}
      {hasChildren && (
        <span className="flex h-4 shrink-0 items-center">
          <ChevronRight
            className={
              "size-3 text-muted-foreground/45 transition-transform" +
              (collapsed ? "" : " rotate-90")
            }
          />
        </span>
      )}
    </div>
  )
}

/** The lucide status glyph for a task, colour-keyed to the app palette. */
function StatusIcon({ status }: { status: ThreadTask["status"] }) {
  if (status === "done") {
    return <SquareCheckBig className="size-3 shrink-0 text-(--ok)" />
  }
  if (status === "in_progress") {
    // A lit segment (the "snake") travels clockwise around a FIXED square
    // outline — the square and its centre dot never move (the old animate-spin
    // rotated the whole glyph, corners and all, which is not what "the border
    // turns around the square" means). Built with SVG stroke-dashoffset
    // marching: `pathLength={100}` normalises the perimeter to 100 units, so the
    // dash `80 20` is an 80%-of-perimeter lit snake + 20% gap, and the
    // `snake-border` keyframe runs the offset 0→-100 for one clockwise lap.
    // A dim full outline underneath is the "track" the snake runs on. The dot
    // is an SVG <circle> so it dodges the global `border-radius:0` reset (a
    // CSS-rounded element would render square). `motion-reduce` drops the snake,
    // leaving the static track + dot.
    return (
      <svg viewBox="0 0 12 12" className="size-3 shrink-0" fill="none" aria-hidden="true">
        <rect
          x="1.25"
          y="1.25"
          width="9.5"
          height="9.5"
          stroke="var(--signal)"
          strokeOpacity="0.25"
          strokeWidth="1.5"
        />
        <rect
          x="1.25"
          y="1.25"
          width="9.5"
          height="9.5"
          stroke="var(--signal)"
          strokeWidth="1.5"
          pathLength={100}
          strokeDasharray="80 20"
          className="motion-reduce:hidden"
          style={{ animation: "snake-border 1.2s linear infinite" }}
        />
        <circle cx="6" cy="6" r="1.25" fill="var(--signal)" />
      </svg>
    )
  }
  return <Square className="size-3 shrink-0 text-(--linear-purple)" />
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
  // A branch with NO started work either (every descendant still `planned`, no
  // done, no in_progress) also folds away — a not-yet-touched group reads calm.
  const allPlanned = (id: string): boolean => {
    const kids = childrenOf.get(id)
    if (!kids) return false // leaf — not a collapsible parent
    return kids.every((k) => k.status === "planned" && (!childrenOf.has(k.id) || allPlanned(k.id)))
  }
  for (const t of tasks) {
    if (childrenOf.has(t.id) && (allClosed(t.id) || allPlanned(t.id))) defaultCollapsed.add(t.id)
  }
  return { childrenOf, defaultCollapsed }
}

/**
 * The Notes-tab body of {@link ThreadAside} (T716) — the focused thread's
 * scratchpad cells, read-only, rendered as a list that expands on click to
 * reveal each cell's full content (the Files-tab interaction pattern, kept
 * inline rather than a separate preview pane).
 *
 * Co-located with {@link TaskList} here (rather than its own file) so the
 * `fileUpload/` directory stays within the 8-entry structure cap — the two are
 * the aside's read-only tab-body list components and share the same imports.
 * The list is projected by the agent (thread-owned scratchpad cells) and rides
 * the live `notes_changed` delta, so this stays purely presentational.
 */
export function NoteList({ notes }: { notes: ThreadNote[] }) {
  // Ids of expanded note rows (content shown). A row starts collapsed; clicking
  // it toggles. A Set (rather than a keyed record) sidesteps the dynamic
  // property-existence lint and reads cleanly.
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set())

  if (notes.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-center text-[11px] text-muted-foreground/45">
        No notes on this thread yet.
      </div>
    )
  }

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  return (
    <div className="space-y-0.5 p-1.5">
      {notes.map((note) => (
        <NoteRow
          key={note.id}
          note={note}
          open={expanded.has(note.id)}
          onToggle={() => toggle(note.id)}
        />
      ))}
    </div>
  )
}

/** One note row: sticky-note icon + title, a trailing expand chevron, and the
 *  content revealed below when open (whitespace preserved, like a note body). */
function NoteRow({
  note,
  open,
  onToggle,
}: {
  note: ThreadNote
  open: boolean
  onToggle: () => void
}) {
  return (
    <div className="rounded-lg">
      <div
        role="button"
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key !== "Enter" && e.key !== " ") return
          e.preventDefault()
          onToggle()
        }}
        className="group flex cursor-pointer items-start gap-1.5 rounded-lg px-2 py-1.5"
      >
        <span className="flex h-4 shrink-0 items-center">
          <StickyNote className="size-3 shrink-0 text-(--linear-purple)" />
        </span>
        <span className="flex min-h-4 min-w-0 flex-1 items-center">
          <span className="truncate text-[13.5px]/none text-foreground/85 group-hover:text-foreground">
            {note.title}
          </span>
        </span>
        <span className="flex h-4 shrink-0 items-center">
          <ChevronRight
            className={
              "size-3 text-muted-foreground/45 transition-transform" + (open ? " rotate-90" : "")
            }
          />
        </span>
      </div>
      {open && (
        <div className="px-2 pt-0.5 pb-2 pl-[1.85rem]">
          <Markdown
            text={note.content}
            className="text-[12.5px] leading-relaxed text-muted-foreground/80"
          />
        </div>
      )}
    </div>
  )
}
