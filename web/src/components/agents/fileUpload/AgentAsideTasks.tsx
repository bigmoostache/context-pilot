import { useMemo, useState, type KeyboardEvent } from "react"
import { ChevronRight, NotepadText } from "lucide-react"
import type { AgentTask, AgentNote } from "@/lib/types"
import { Markdown } from "@/lib/support/markdown"

/**
 * The Tasks-tab body of {@link AgentAside} (T662) — the agent's todo tree,
 * read-only: an overall progress header, then indented rows with one status
 * icon family. Parents show their subtree's done/total and fold on click; a
 * parent whose subtree is all done or all planned starts folded.
 *
 * The tree is projected by the agent (cancelled excluded upstream) and rides the
 * live `task_list_changed` delta, so this stays purely presentational.
 */
export function TaskList({ tasks }: { tasks: AgentTask[] }) {
  // Parent → children buckets, per-parent progress, and the ids whose subtree
  // defaults to collapsed (all finished, or none started).
  const model = useMemo(() => buildModel(tasks), [tasks])

  // User overrides on the default collapsed state, keyed by task id. Effective
  // collapsed = override ?? default. A parent not in the map follows its default.
  const [overrides, setOverrides] = useState<Record<string, boolean>>({})

  if (tasks.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-center text-[11px] text-muted-foreground/45">
        No tasks on this agent yet.
      </div>
    )
  }

  const toggle = (id: string, currentlyCollapsed: boolean) =>
    setOverrides((o) => ({ ...o, [id]: !currentlyCollapsed }))

  // Pre-order walk, skipping the subtree of any collapsed parent.
  const rows: { task: AgentTask; depth: number; collapsed: boolean }[] = []
  const walk = (parentKey: string, depth: number) => {
    const kids = model.childrenOf.get(parentKey) ?? []
    for (const t of kids) {
      const parent = model.childrenOf.has(t.id)
      const collapsed = parent && (overrides[t.id] ?? model.defaultCollapsed.has(t.id))
      rows.push({ task: t, depth, collapsed })
      if (parent && !collapsed) walk(t.id, depth + 1)
    }
  }
  walk(ROOT_KEY, 0)

  const done = tasks.filter((t) => t.status === "done").length

  return (
    <div className="px-1.5 pt-2 pb-3">
      <ProgressHeader done={done} total={tasks.length} />
      <div>
        {rows.map(({ task, depth, collapsed }) => (
          <TaskRow
            key={task.id}
            task={task}
            depth={depth}
            progress={model.progress.get(task.id)}
            collapsed={collapsed}
            onToggle={() => toggle(task.id, collapsed)}
          />
        ))}
      </div>
    </div>
  )
}

/** "3 of 8 done" over a hairline progress bar (the tab already says "Tasks"). Static. */
function ProgressHeader({ done, total }: { done: number; total: number }) {
  return (
    <div className="mb-2 px-2">
      <div className="text-right text-[11px] text-foreground/75 tabular-nums">
        {done} of {total} done
      </div>
      <div className="mt-1.5 h-[2px] overflow-hidden rounded-full bg-foreground/8">
        <div
          className="h-full rounded-full bg-(--ok)"
          style={{ width: `${String((done / Math.max(total, 1)) * 100)}%` }}
        />
      </div>
    </div>
  )
}

/** Row geometry, in px: horizontal padding, per-level indent, icon box. */
const PAD = 8
const INDENT = 16
const ICON = 14

/**
 * One task row: status icon, name (wraps; first line aligned on the icon),
 * and for parents a trailing "done/total" count + chevron. Nested rows draw a
 * hairline guide under each ancestor's icon so the tree reads at a glance.
 */
function TaskRow({
  task,
  depth,
  progress,
  collapsed,
  onToggle,
}: {
  task: AgentTask
  depth: number
  progress: Progress | undefined
  collapsed: boolean
  onToggle: () => void
}) {
  const parent = progress !== undefined
  // No greying: every state stays fully readable; the icon carries the status.
  const text = task.status === "in_progress" ? "font-medium text-foreground" : "text-foreground/90"

  return (
    <div
      className={
        "relative flex items-start gap-2 rounded-md py-[5px] pr-2" +
        (parent
          ? " group/row cursor-pointer transition-colors duration-150 hover:bg-muted/50 active:bg-muted/70"
          : "")
      }
      style={{ paddingLeft: PAD + depth * INDENT }}
      title={task.description ?? undefined}
      {...(parent
        ? {
            role: "button" as const,
            tabIndex: 0,
            "aria-expanded": !collapsed,
            onClick: onToggle,
            onKeyDown: (e: KeyboardEvent) => {
              if (e.key !== "Enter" && e.key !== " ") return
              e.preventDefault()
              onToggle()
            },
          }
        : {})}
    >
      {Array.from({ length: depth }, (_, i) => (
        <span
          key={i}
          aria-hidden="true"
          className="absolute inset-y-0 w-px bg-foreground/8"
          style={{ left: PAD + i * INDENT + ICON / 2 }}
        />
      ))}
      <span className="flex h-[18px] shrink-0 items-center">
        <StatusIcon status={task.status} />
      </span>
      <span className={"min-w-0 flex-1 text-[12.5px]/[18px] wrap-break-word " + text}>
        {task.name}
      </span>
      {parent && (
        <span className="flex h-[18px] shrink-0 items-center gap-1 text-[10.5px] text-foreground/65 tabular-nums transition-colors group-hover/row:text-foreground/85">
          {progress.done}/{progress.total}
          <ChevronRight
            className={
              "size-3 text-foreground/55 transition-[transform,color] duration-150 group-hover/row:text-foreground/85" +
              (collapsed ? "" : " rotate-90")
            }
          />
        </span>
      )}
    </div>
  )
}

/**
 * The status glyph: ONE family for all three states — the same 14px circle,
 * same 1.5 stroke, differing only in fill. Planned = empty ring, in progress =
 * ring with a half-filled core, done = filled disc with a check.
 */
function StatusIcon({ status }: { status: AgentTask["status"] }) {
  const ring = { cx: 7, cy: 7, r: 5.75, strokeWidth: 1.5 }
  return (
    <svg
      viewBox="0 0 14 14"
      className="size-[14px] shrink-0"
      fill="none"
      aria-label={STATUS_LABEL[status]}
      role="img"
    >
      {status === "planned" && (
        <circle {...ring} stroke="currentColor" className="text-foreground/45" />
      )}
      {status === "in_progress" && (
        <>
          <circle {...ring} stroke="var(--signal)" />
          <path d="M7 3.5 A3.5 3.5 0 0 1 7 10.5 Z" fill="var(--signal)" />
        </>
      )}
      {status === "done" && (
        <>
          <circle {...ring} stroke="var(--ok)" fill="var(--ok)" />
          <path
            d="M4.5 7.2 L6.25 8.9 L9.5 5.4"
            stroke="var(--background)"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </>
      )}
    </svg>
  )
}

const STATUS_LABEL: Record<AgentTask["status"], string> = {
  planned: "Planned",
  in_progress: "In progress",
  done: "Done",
}

const ROOT_KEY = "\u{0}root"

/** Finished vs total descendants of a parent task. */
interface Progress {
  done: number
  total: number
}

interface TaskModel {
  /** parentKey → ordered children (ROOT_KEY for top-level). */
  childrenOf: Map<string, AgentTask[]>
  /** parent id → done/total over its whole subtree. */
  progress: Map<string, Progress>
  /** ids of parents whose subtree is all done, or all planned (default-collapsed). */
  defaultCollapsed: Set<string>
}

/**
 * Build the parent→children map (insertion order preserved), each parent's
 * subtree progress, and the default-collapsed set: a parent folds away when its
 * whole subtree is done (finished work) or all planned (untouched work). A
 * task whose `parentId` is absent or points outside this agent is a root.
 */
function buildModel(tasks: AgentTask[]): TaskModel {
  const ids = new Set(tasks.map((t) => t.id))
  const childrenOf = new Map<string, AgentTask[]>()
  for (const t of tasks) {
    const key = t.parentId && ids.has(t.parentId) ? t.parentId : ROOT_KEY
    const bucket = childrenOf.get(key)
    if (bucket) bucket.push(t)
    else childrenOf.set(key, [t])
  }

  const progress = new Map<string, Progress>()
  const defaultCollapsed = new Set<string>()
  // Returns the subtree's counts plus whether every descendant is done / planned.
  const visit = (id: string): Progress & { allDone: boolean; allPlanned: boolean } => {
    let done = 0
    let total = 0
    let allDone = true
    let allPlanned = true
    const kids = childrenOf.get(id) ?? []
    for (const k of kids) {
      total += 1
      if (k.status === "done") done += 1
      allDone &&= k.status === "done"
      allPlanned &&= k.status === "planned"
      if (childrenOf.has(k.id)) {
        const sub = visit(k.id)
        done += sub.done
        total += sub.total
        allDone &&= sub.allDone
        allPlanned &&= sub.allPlanned
      }
    }
    if (id !== ROOT_KEY) {
      progress.set(id, { done, total })
      if (allDone || allPlanned) defaultCollapsed.add(id)
    }
    return { done, total, allDone, allPlanned }
  }
  visit(ROOT_KEY)
  return { childrenOf, progress, defaultCollapsed }
}

/**
 * The Notes-tab body of {@link AgentAside} (T716) — the focused agent's
 * scratchpad cells, read-only. A list of cards (title + two-line excerpt);
 * picking one opens {@link NotePreview} in the widened rail, the same
 * list → preview flow as the Files tab.
 *
 * Co-located with {@link TaskList} so `fileUpload/` stays within the 8-entry
 * structure cap. The list rides the live `notes_changed` delta.
 */
export function NoteList({
  notes,
  onSelect,
}: {
  notes: AgentNote[]
  onSelect: (id: string) => void
}) {
  if (notes.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-center text-[11px] text-muted-foreground/45">
        No notes on this agent yet.
      </div>
    )
  }

  return (
    <div className="px-1.5 pt-2 pb-3">
      {notes.map((note) => (
        <button
          key={note.id}
          type="button"
          onClick={() => onSelect(note.id)}
          className="group/note flex w-full cursor-pointer items-start gap-2 rounded-md px-2 py-[7px] text-left transition-colors duration-150 hover:bg-muted/50 active:bg-muted/70"
        >
          <span className="flex h-[18px] shrink-0 items-center">
            <NotepadText className="size-[14px] text-foreground/55 transition-colors group-hover/note:text-foreground/85" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[12.5px]/[18px] font-medium text-foreground">
              {note.title}
            </span>
            {excerpt(note.content) && (
              <span className="mt-0.5 line-clamp-2 text-[11.5px]/[16px] text-foreground/65 transition-colors group-hover/note:text-foreground/80">
                {excerpt(note.content)}
              </span>
            )}
          </span>
          <ChevronRight className="mt-[3px] size-3 shrink-0 -translate-x-0.5 text-foreground/55 opacity-0 transition-[opacity,transform] duration-200 group-hover/note:translate-x-0 group-hover/note:opacity-100 motion-reduce:transition-none" />
        </button>
      ))}
    </div>
  )
}

/** Plain-text excerpt for a note card: markdown markers dropped, whitespace collapsed. */
function excerpt(md: string): string {
  return md
    .replaceAll(/```[\s\S]*?```/g, " ")
    .replaceAll(/[#>*_`|[\]]/g, "")
    .replaceAll(/\s+/g, " ")
    .trim()
    .slice(0, 240)
}

/** One note, full width of the widened rail: title, then the rendered markdown body. */
export function NotePreview({ note }: { note: AgentNote }) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 pt-4 pb-6">
      <h2 className="mb-3 flex items-center gap-2 text-[15px] font-semibold text-foreground">
        <NotepadText className="size-4 shrink-0 text-foreground/55" />
        <span className="min-w-0 wrap-break-word">{note.title}</span>
      </h2>
      <Markdown text={note.content} className="text-[13px] leading-relaxed text-foreground/90" />
    </div>
  )
}
