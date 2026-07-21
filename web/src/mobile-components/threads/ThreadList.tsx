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
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"
import { clickable } from "@/lib/support/a11y"
import { previewOf } from "@/lib/support/threadMessages"

interface ThreadListProps {
  /** all of the realm's threads (archived included) — filtering happens here */
  threads: ThreadDetail[]
  selectedId: string
  onSelect: (id: string) => void
  /** live search query (controlled by the parent so it survives navigation) */
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
 * Mobile thread roster — the divergent twin of `components/threads/ThreadList`.
 *
 * The desktop version is a fixed `--sidebar-w` rail sitting beside the
 * conversation; on a phone the list is a **full-width screen** in the thread
 * stack (the conversation is a separate pushed screen, see the mobile
 * `ThreadsView`). Two touch-driven changes fall out of that:
 *
 *   • **Full width** — no `--sidebar-w` / `border-r`; the list owns the viewport.
 *   • **Always-visible row actions** — a touch device has no hover, so the
 *     archive/pause/delete controls can't hide behind `group-hover`. They sit
 *     inline at a comfortable 32px tap size instead of the desktop's 20px
 *     hover-revealed cluster.
 *
 * All filtering / grouping / sorting logic is identical to desktop — only the
 * chrome and touch affordances fork.
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

  /** Focused-first, then most-recent (T36) — the actively-worked thread floats
   *  to the top of the agent-turn group regardless of last-activity time. */
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
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface">
      <ListHeader
        showArchived={showArchived}
        onToggleArchived={onToggleArchived}
        liveCount={live.length}
        archivedCount={archivedCount}
        workingCount={workingCount}
      />

      {/* new thread (hidden in archived view — archived is read-only) */}
      {!showArchived && (
        <div className="shrink-0 px-3 pb-2">
          <button
            onClick={onNewThread}
            className="flex w-full items-center justify-center gap-2 rounded-lg bg-(--signal) px-3 py-2.5 text-[13px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105"
          >
            <Plus className="size-4" />
            New Thread
          </button>
        </div>
      )}

      {/* search — works in both live and archived views */}
      <div className="shrink-0 px-3 pb-2">
        <div className="flex items-center gap-2 rounded-lg border border-border bg-card px-2.5 py-2 text-[13px] focus-within:border-(--signal)/60">
          <Search className="size-4 shrink-0 text-muted-foreground/60" />
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
              <X className="size-4" />
            </button>
          )}
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="px-2 py-1">
          {visible.length === 0 && <EmptyState hasQuery={q !== ""} showArchived={showArchived} />}

          {!showArchived && (
            <>
              {mine.length > 0 && <Group label="Agent's turn" count={mine.length} />}
              {mine.map((t) => row(t))}

              {working.length > 0 && <Group label="User turn" count={working.length} />}
              {working.map((t) => row(t))}
            </>
          )}

          {showArchived && [...visible].toSorted(byRecent).map((t) => row(t, true))}
        </div>
      </ScrollArea>

      {/* archived entry point (hidden while already viewing archived) */}
      {!showArchived && archivedCount > 0 && (
        <button
          onClick={() => onToggleArchived(true)}
          className="flex shrink-0 items-center gap-2 border-t border-border px-3.5 py-3 text-[13px] text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground"
        >
          <Archive className="size-4" />
          Archived
          <span className="ml-auto text-muted-foreground/50 tabular-nums">{archivedCount}</span>
        </button>
      )}
    </div>
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
          className="flex items-center gap-1.5 text-[13px] font-medium text-foreground/80 transition-colors hover:text-foreground"
        >
          <ChevronLeft className="size-4" />
          Archived
          <span className="text-muted-foreground/50 tabular-nums">{archivedCount}</span>
        </button>
      ) : (
        <>
          <span className="text-[12px] text-muted-foreground tabular-nums">
            {liveCount} thread{liveCount === 1 ? "" : "s"}
          </span>
          {workingCount > 0 && (
            <span
              className="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium"
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
  return <p className="px-2.5 py-6 text-center text-[12.5px] text-muted-foreground/55">{message}</p>
}

function Group({ label, count, accent }: { label: string; count: number; accent?: string }) {
  return (
    <div className="flex items-center gap-2 px-2.5 pt-3 pb-1">
      <span
        className="text-[12px] font-semibold"
        style={{ color: accent ?? "var(--muted-foreground)" }}
      >
        {label}
      </span>
      <span className="text-[12px] text-muted-foreground/45 tabular-nums">{count}</span>
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
        "relative flex w-full flex-col gap-1 rounded-lg p-3 text-left transition-colors",
        selected ? "card-shadow bg-card" : "active:bg-muted/60",
      )}
    >
      <div
        {...clickable(() => onSelect(t.id))}
        className="flex cursor-pointer flex-col gap-1 text-left"
      >
        {/* line 1 — dot + name + time */}
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
          <span className="truncate text-[14px] font-medium text-foreground/90">{t.name}</span>
          <span className="ml-auto shrink-0 text-[11px] text-muted-foreground/50 tabular-nums">
            {t.lastActivity}
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
      {/* Touch has no hover — the row actions sit inline, always visible, at a
          comfortable tap size (the desktop twin hides them behind group-hover). */}
      <RowActions
        id={t.id}
        archived={Boolean(archived)}
        isPaused={Boolean(isPaused)}
        onArchive={onArchive}
        onDelete={onDelete}
        onPause={onPause}
      />
    </div>
  )
}

/** The row's action cluster: archive/restore, optional permanent-delete
 *  (archived rows) and optional pause/resume (live rows). Each button stops
 *  propagation so it doesn't also open the thread. Always visible (no hover on
 *  touch), 32px tap targets. */
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
    <div className="flex items-center justify-end gap-1 pl-4">
      <button
        onClick={(e) => {
          e.stopPropagation()
          onArchive(id)
        }}
        className="flex size-8 items-center justify-center rounded-md text-muted-foreground/70 active:bg-muted"
        title={archived ? "Restore" : "Archive"}
      >
        {archived ? <ArchiveRestore className="size-4" /> : <Archive className="size-4" />}
      </button>
      {archived && onDelete && (
        <button
          onClick={(e) => {
            e.stopPropagation()
            onDelete(id)
          }}
          className="flex size-8 items-center justify-center rounded-md text-muted-foreground/70 active:bg-muted active:text-(--danger)"
          title="Delete permanently"
        >
          <Trash2 className="size-4" />
        </button>
      )}
      {!archived && onPause && (
        <button
          onClick={(e) => {
            e.stopPropagation()
            onPause(id)
          }}
          className="flex size-8 items-center justify-center rounded-md text-muted-foreground/70 active:bg-muted"
          title={isPaused ? "Resume" : "Pause"}
        >
          {isPaused ? <Play className="size-4" /> : <Pause className="size-4" />}
        </button>
      )}
    </div>
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
      <span className="truncate text-[12.5px] text-muted-foreground/70">{preview}</span>
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
