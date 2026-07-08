import {
  Plus,
  Search,
  X,
  Archive,
  ArchiveRestore,
  ChevronLeft,
  Pause,
  Play,
  Trash2,
} from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"
import { clickable } from "@/lib/support/a11y"
import { previewOf } from "@/lib/support/threadMessages"

interface ThreadListProps {
  /** all of the realm's threads (archived included) — filtering happens here */
  threads: ThreadDetail[]
  selectedId: string
  onSelect: (id: string) => void
  /** live search query (controlled by the parent so it survives collapse) */
  query: string
  onQueryChange: (q: string) => void
  /** archived view toggle */
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  /** archive ↔ restore a single thread */
  onArchive: (id: string) => void
  /** permanently delete a thread (T371) */
  onDelete: (id: string) => void
  /** pause ↔ resume a single thread (T371) */
  onPause: (id: string) => void
  /** open the New Thread dialog */
  onNewThread: () => void
}

/** Sort threads by most recent activity first. */
function byRecent(a: ThreadDetail, b: ThreadDetail): number {
  return (b.lastActivityMs ?? 0) - (a.lastActivityMs ?? 0)
}

/**
 * Left rail of the thread-centered view — a clean, grouped chat sidebar.
 *
 * The agent identity (name / folder / logo) is intentionally *not* repeated
 * here: it already lives in the TopBar. The rail is **always open** — there is
 * no collapse affordance (removed per T23).
 *
 * The search box genuinely filters (by name + last-message preview). Threads
 * are grouped by turn-status — **Needs you** (MY_TURN), **Active** (the single
 * green thread the agent is streaming right now), **Working in parallel**
 * (THEIR_TURN) — with an on-demand **Archived** view. Its width is the shared
 * `--sidebar-w` CSS variable so it lines up with every other rail.
 *
 * Structure (P8): the context-sensitive top bar and the empty placeholder are
 * factored into {@link ListHeader} / {@link EmptyState}, and each row's hover
 * actions + badges into {@link RowActions} / {@link RowMeta}, so this component
 * and {@link ThreadRow} both stay within the complexity/line budgets.
 */
export function ThreadList({
  threads,
  selectedId,
  onSelect,
  query,
  onQueryChange,
  showArchived,
  onToggleArchived,
  onArchive,
  onDelete,
  onPause,
  onNewThread,
}: ThreadListProps) {
  const q = query.trim().toLowerCase()
  const matches = (t: ThreadDetail) =>
    q === "" || t.name.toLowerCase().includes(q) || previewOf(t).toLowerCase().includes(q)

  const live = threads.filter((t) => !t.archived)
  const archived = threads.filter((t) => t.archived)
  const archivedCount = archived.length

  // search applies to whichever set is on screen
  const visible = (showArchived ? archived : live).filter((t) => matches(t))

  /**
   * Sort the "Agent's turn" group focused-first, then by recency (T36). The
   * thread the agent is actively focused on (`focused_thread_id`, surfaced as
   * `t.focused`) is the one most worth seeing at a glance, so it floats to the
   * top of the section regardless of last-activity time.
   */
  const byFocusThenRecent = (a: ThreadDetail, b: ThreadDetail) => {
    const fa = a.focused ? 1 : 0
    const fb = b.focused ? 1 : 0
    if (fa !== fb) return fb - fa
    return byRecent(a, b)
  }

  const mine = visible.filter((t) => t.status === "MY_TURN").toSorted(byFocusThenRecent)
  const working = visible
    .filter((t) => t.status === "THEIR_TURN" || t.status === "ACTIVE")
    .toSorted(byRecent)
  // agent-owned, actively-or-parallel working count (for the header pill)
  const workingCount = live.filter((t) => t.status !== "MY_TURN").length

  const row = (t: ThreadDetail, archivedRow?: boolean) => (
    <ThreadRow
      key={t.id}
      t={t}
      selected={t.id === selectedId}
      onSelect={onSelect}
      onArchive={onArchive}
      onPause={archivedRow ? undefined : onPause}
      onDelete={archivedRow ? onDelete : undefined}
      archived={archivedRow}
    />
  )

  return (
    <aside className="flex w-(--sidebar-w) shrink-0 flex-col overflow-hidden border-r border-border bg-surface">
      {/* fixed-width inner shell pinned to the rail width */}
      <div
        className="flex h-full flex-col"
        style={{ width: "var(--sidebar-w)", minWidth: "var(--sidebar-w)" }}
      >
        <ListHeader
          showArchived={showArchived}
          onToggleArchived={onToggleArchived}
          liveCount={live.length}
          archivedCount={archivedCount}
          workingCount={workingCount}
        />

        {/* new thread + search (hidden in archived view — archived is read-only) */}
        {!showArchived && (
          <div className="shrink-0 px-3 pb-2">
            <button
              onClick={onNewThread}
              className="flex w-full items-center justify-center gap-2 rounded-lg bg-(--signal) px-3 py-2 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105"
            >
              <Plus className="size-4" />
              New Thread
            </button>
          </div>
        )}

        {/* search — works in both live and archived views */}
        <div className="shrink-0 px-3 pb-2">
          <div className="flex items-center gap-2 rounded-lg border border-border bg-card px-2.5 py-1.5 text-[12px] focus-within:border-(--signal)/60">
            <Search className="size-3.5 shrink-0 text-muted-foreground/60" />
            <input
              value={query}
              onChange={(e) => onQueryChange(e.target.value)}
              placeholder={showArchived ? "Search archived…" : "Search threads…"}
              className="min-w-0 flex-1 bg-transparent text-foreground/90 outline-none placeholder:text-muted-foreground/55"
            />
            {query && (
              <button
                onClick={() => onQueryChange("")}
                className="shrink-0 text-muted-foreground/55 transition-colors hover:text-foreground"
                title="Clear"
              >
                <X className="size-3.5" />
              </button>
            )}
          </div>
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <div className="px-2 py-1">
            {visible.length === 0 && <EmptyState hasQuery={q !== ""} showArchived={showArchived} />}

            {!showArchived && (
              <>
                {working.length > 0 && <Group label="User turn" count={working.length} />}
                {working.map((t) => row(t))}

                {mine.length > 0 && <Group label="Agent's turn" count={mine.length} />}
                {mine.map((t) => row(t))}
              </>
            )}

            {showArchived &&
              // Latest-archived first (T277) — most recently active on top.
              [...visible].toSorted(byRecent).map((t) => row(t, true))}
          </div>
        </ScrollArea>

        {/* archived entry point (hidden while already viewing archived) */}
        {!showArchived && archivedCount > 0 && (
          <button
            onClick={() => onToggleArchived(true)}
            className="flex shrink-0 items-center gap-2 border-t border-border px-3.5 py-2.5 text-[12px] text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground"
          >
            <Archive className="size-3.5" />
            Archived
            <span className="ml-auto text-muted-foreground/50 tabular-nums">{archivedCount}</span>
          </button>
        )}
      </div>
    </aside>
  )
}

/** Context-sensitive top bar: the live thread count + parallelism pill, or an
 *  "Archived ‹back›" header while viewing the archived set. */
function ListHeader({
  showArchived,
  onToggleArchived,
  liveCount,
  archivedCount,
  workingCount,
}: {
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  liveCount: number
  archivedCount: number
  workingCount: number
}) {
  return (
    <div className="flex items-center gap-2 px-3 pt-3 pb-2.5">
      {showArchived ? (
        <button
          onClick={() => onToggleArchived(false)}
          className="flex items-center gap-1.5 text-[12px] font-medium text-foreground/80 transition-colors hover:text-foreground"
        >
          <ChevronLeft className="size-3.5" />
          Archived
          <span className="text-muted-foreground/50 tabular-nums">{archivedCount}</span>
        </button>
      ) : (
        <>
          <span className="text-[11px] text-muted-foreground tabular-nums">
            {liveCount} thread{liveCount === 1 ? "" : "s"}
          </span>
          {workingCount > 0 && (
            <span
              className="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[10.5px] font-medium"
              style={{
                background: "color-mix(in oklab, var(--interactive) 14%, transparent)",
                color: "var(--interactive)",
              }}
            >
              <span className="relative flex size-1.5">
                <span className="absolute inline-flex size-full animate-ping rounded-full bg-(--interactive) opacity-70" />
                <span className="relative inline-flex size-1.5 rounded-full bg-(--interactive)" />
              </span>
              {workingCount} working
            </span>
          )}
        </>
      )}
    </div>
  )
}

/** The empty placeholder shown when no thread is visible — copy adapts to
 *  whether a search is active and which set (live/archived) is on screen. */
function EmptyState({ hasQuery, showArchived }: { hasQuery: boolean; showArchived: boolean }) {
  const message = hasQuery
    ? "No threads match your search."
    : showArchived
      ? "No archived threads."
      : "No threads yet."
  return <p className="px-2.5 py-6 text-center text-[11.5px] text-muted-foreground/55">{message}</p>
}

function Group({ label, count, accent }: { label: string; count: number; accent?: string }) {
  return (
    <div className="flex items-center gap-2 px-2.5 pt-3 pb-1">
      <span
        className="text-[11px] font-semibold"
        style={{ color: accent ?? "var(--muted-foreground)" }}
      >
        {label}
      </span>
      <span className="text-[11px] text-muted-foreground/45 tabular-nums">{count}</span>
    </div>
  )
}

/** The status-dot colour for a thread row: green when focused/active, signal
 *  when it's your turn, muted otherwise. A flat if-chain, not a nested ternary. */
function dotColor(isFocused: boolean, status: ThreadDetail["status"]): string {
  if (isFocused) return "var(--ok)"
  if (status === "MY_TURN") return "var(--signal)"
  if (status === "ACTIVE") return "var(--ok)"
  return "var(--muted-foreground)"
}

function ThreadRow({
  t,
  selected,
  onSelect,
  onArchive,
  onPause,
  onDelete,
  archived,
}: {
  t: ThreadDetail
  selected: boolean
  onSelect: (id: string) => void
  onArchive: (id: string) => void
  onPause?: ((id: string) => void) | undefined
  onDelete?: ((id: string) => void) | undefined
  archived?: boolean | undefined
}) {
  const isFocused = !archived && t.focused
  const isPaused = !archived && t.paused
  const dot = dotColor(Boolean(isFocused), t.status)
  const pulse = isFocused || t.status === "MY_TURN" || t.status === "ACTIVE"

  return (
    <div
      className={cn(
        "group relative flex w-full flex-col gap-1 rounded-lg px-2.5 py-2 text-left transition-colors",
        selected ? "card-shadow bg-card" : "hover:bg-muted/60",
      )}
    >
      <div
        {...clickable(() => onSelect(t.id))}
        className="flex cursor-pointer flex-col gap-1 text-left"
      >
        {/* line 1 — dot + name + time + hover actions */}
        <div className="flex items-center gap-2">
          <span
            className={cn(
              "size-2 shrink-0 rounded-full",
              pulse && !archived && !isPaused && "animate-pulse",
            )}
            style={{
              background: archived ? "var(--muted-foreground)" : isPaused ? "var(--warn)" : dot,
            }}
          />
          <span className="truncate text-[13px] font-medium text-foreground/90">{t.name}</span>
          <span className="relative ml-auto shrink-0">
            <span className="text-[10.5px] text-muted-foreground/50 tabular-nums transition-opacity group-hover:opacity-0">
              {t.lastActivity}
            </span>
            <RowActions
              id={t.id}
              archived={Boolean(archived)}
              isPaused={Boolean(isPaused)}
              onArchive={onArchive}
              onDelete={onDelete}
              onPause={onPause}
            />
          </span>
        </div>
        {/* line 2 — badges + preview + unread */}
        <RowMeta
          t={t}
          archived={Boolean(archived)}
          isFocused={Boolean(isFocused)}
          isPaused={Boolean(isPaused)}
        />
      </div>
    </div>
  )
}

/** The hover-revealed action cluster on a row's first line: archive/restore,
 *  optional permanent-delete (archived rows) and optional pause/resume (live
 *  rows). Each button stops propagation so it doesn't also select the row. */
function RowActions({
  id,
  archived,
  isPaused,
  onArchive,
  onDelete,
  onPause,
}: {
  id: string
  archived: boolean
  isPaused: boolean
  onArchive: (id: string) => void
  onDelete?: ((id: string) => void) | undefined
  onPause?: ((id: string) => void) | undefined
}) {
  return (
    <span className="absolute inset-0 flex items-center justify-end gap-1 opacity-0 transition-opacity group-hover:opacity-100">
      <button
        onClick={(e) => {
          e.stopPropagation()
          onArchive(id)
        }}
        className="flex size-5 items-center justify-center rounded-md text-muted-foreground/60 hover:bg-muted hover:text-foreground"
        title={archived ? "Restore" : "Archive"}
      >
        {archived ? <ArchiveRestore className="size-3" /> : <Archive className="size-3" />}
      </button>
      {archived && onDelete && (
        <button
          onClick={(e) => {
            e.stopPropagation()
            onDelete(id)
          }}
          className="flex size-5 items-center justify-center rounded-md text-muted-foreground/60 hover:bg-muted hover:text-(--danger)"
          title="Delete permanently"
        >
          <Trash2 className="size-3" />
        </button>
      )}
      {!archived && onPause && (
        <button
          onClick={(e) => {
            e.stopPropagation()
            onPause(id)
          }}
          className="flex size-5 items-center justify-center rounded-md text-muted-foreground/60 hover:bg-muted hover:text-foreground"
          title={isPaused ? "Resume" : "Pause"}
        >
          {isPaused ? <Play className="size-3" /> : <Pause className="size-3" />}
        </button>
      )}
    </span>
  )
}

/** A row's second line: focused / paused status badges, the flattened preview
 *  snippet, and the unread-count pill. */
function RowMeta({
  t,
  archived,
  isFocused,
  isPaused,
}: {
  t: ThreadDetail
  archived: boolean
  isFocused: boolean
  isPaused: boolean
}) {
  const preview = previewOf(t)
  return (
    <div className="flex items-center gap-1.5 pl-4">
      {isFocused && (
        <span
          className="shrink-0 rounded-full px-1.5 py-px text-[9.5px] font-semibold tracking-wide uppercase"
          style={{
            background: "color-mix(in oklab, var(--ok) 18%, transparent)",
            color: "var(--ok)",
          }}
        >
          focused
        </span>
      )}
      {isPaused && (
        <span
          className="shrink-0 rounded-full px-1.5 py-px text-[9.5px] font-semibold tracking-wide uppercase"
          style={{
            background: "color-mix(in oklab, var(--warn) 18%, transparent)",
            color: "var(--warn)",
          }}
        >
          paused
        </span>
      )}
      <span className="truncate text-[11.5px] text-muted-foreground/70">{preview}</span>
      {!archived && (t.unread ?? 0) > 0 && (
        <span
          className="ml-auto shrink-0 rounded-full px-1.5 text-[10px] font-semibold text-(--primary-foreground) tabular-nums"
          style={{ background: "var(--signal)" }}
        >
          {t.unread}
        </span>
      )}
    </div>
  )
}
