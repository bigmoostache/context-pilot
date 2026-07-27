import {
  Plus,
  Search,
  Archive,
  ArchiveRestore,
  ChevronLeft,
  PanelLeftClose,
  Pause,
  Play,
  Trash2,
} from "lucide-react"
import { useRef, useState } from "react"
import { ScrollArea } from "@/components/ui/scroll-area"
import type { ThreadDetail } from "@/lib/types"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { clickable } from "@/lib/support/a11y"
import { previewOf } from "@/lib/support/threadMessages"
import { ThreadSearchPalette } from "./dialogs/ThreadSearchPalette"

interface ThreadListProps {
  /** all of the realm's threads (archived included) — filtering happens here */
  threads: ThreadDetail[]
  /** owning agent — the conversation-search target for the palette (T671) */
  agentId: string
  selectedId: string
  onSelect: (id: string) => void
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  onArchive: (id: string) => void
  /** permanently delete a thread (T371) */
  onDelete: (id: string) => void
  /** pause ↔ resume a single thread (T371) */
  onPause: (id: string) => void
  onNewThread: () => void
  onToggleSidebar: () => void
}

/** Sort threads by most recent activity first. */
function byRecent(a: ThreadDetail, b: ThreadDetail): number {
  return (b.lastActivityMs ?? 0) - (a.lastActivityMs ?? 0)
}

/**
 * Left rail of the thread-centered view — grouped chat sidebar. Agent identity
 * lives in the TopBar; the rail collapses via the header button (T669). Threads
 * group by turn-status (**Agent's turn** / **User turn**) with an **Archived**
 * view; search moved to the {@link ThreadSearchPalette} command palette. Width =
 * shared `--sidebar-w`. Structure (P8): {@link ListHeader} / {@link EmptyState}
 * / {@link RowActions} / {@link RowMeta} keep budgets.
 */
export function ThreadList({
  threads,
  agentId,
  selectedId,
  onSelect,
  showArchived,
  onToggleArchived,
  onArchive,
  onDelete,
  onPause,
  onNewThread,
  onToggleSidebar,
}: ThreadListProps) {
  const [searchOpen, setSearchOpen] = useState(false)

  const live = threads.filter((t) => !t.archived)
  const archived = threads.filter((t) => t.archived)
  const archivedCount = archived.length

  // the group that's on screen (search now lives in the command palette)
  const visible = showArchived ? archived : live

  /**
   * Sort the "Agent's turn" group focused-first, then by recency (T36). The
   * focused thread (`focused_thread_id`, surfaced as `t.focused`) is the most
   * worth seeing, so it floats to the top regardless of last-activity time.
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
          onOpenSearch={() => setSearchOpen(true)}
          onToggleSidebar={onToggleSidebar}
        />

        {/* new thread (hidden in archived view — archived is read-only) */}
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

        <ScrollArea className="min-h-0 flex-1">
          <div className="px-2 py-1">
            {visible.length === 0 && <EmptyState showArchived={showArchived} />}

            {!showArchived && (
              <>
                {mine.length > 0 && <Group label="Agent's turn" count={mine.length} />}
                {mine.map((t) => row(t))}

                {working.length > 0 && <Group label="User turn" count={working.length} />}
                {working.map((t) => row(t))}
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

      <ThreadSearchPalette
        open={searchOpen}
        onClose={() => setSearchOpen(false)}
        threads={threads}
        agentId={agentId}
        onSelect={(id) => {
          onSelect(id)
          setSearchOpen(false)
        }}
      />
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
  onOpenSearch,
  onToggleSidebar,
}: {
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  liveCount: number
  archivedCount: number
  workingCount: number
  onOpenSearch: () => void
  onToggleSidebar: () => void
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
      {/* right-aligned chrome: open search palette + collapse the rail */}
      <div className="ml-auto flex items-center gap-1">
        <button
          onClick={onOpenSearch}
          title="Search threads"
          className="flex size-6 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
        >
          <Search className="size-3.5" />
        </button>
        <button
          onClick={onToggleSidebar}
          title="Hide sidebar"
          className="flex size-6 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
        >
          <PanelLeftClose className="size-3.5" />
        </button>
      </div>
    </div>
  )
}

/** The empty placeholder shown when no thread is visible — copy adapts to which
 *  set (live/archived) is on screen. */
function EmptyState({ showArchived }: { showArchived: boolean }) {
  const message = showArchived ? "No archived threads." : "No threads yet."
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

/** Row-hover title marquee (WAA): 0.3s dwell, scroll left at 10 chars/s, 0.3s dwell, teleport back.
 *  Returns text-track ref + row hover handlers; no-op if the title fits or prefers-reduced-motion. */
function useTitleMarquee() {
  const trackRef = useRef<HTMLSpanElement>(null)
  const animRef = useRef<Animation | null>(null)
  const onMouseEnter = () => {
    const el = trackRef.current
    if (!el || prefersReducedMotion()) return
    const dist = Math.max(0, el.scrollWidth - el.clientWidth)
    if (dist === 0) return
    const scrollS = dist / (el.scrollWidth / Math.max(1, el.textContent.length)) / 10
    const total = 0.6 + scrollS
    animRef.current = el.animate(
      [
        { transform: "translateX(0)", offset: 0 },
        { transform: "translateX(0)", offset: 0.3 / total },
        { transform: `translateX(-${dist}px)`, offset: (0.3 + scrollS) / total },
        { transform: `translateX(-${dist}px)`, offset: 1 },
      ],
      { duration: total * 1000, iterations: Infinity, easing: "linear" },
    )
  }
  const onMouseLeave = () => animRef.current?.cancel()
  return { trackRef, onMouseEnter, onMouseLeave }
}

/** Thread title markup — ellipsis at rest, brightens + un-clips on row hover; motion via {@link useTitleMarquee}. */
function MarqueeTitle({ name, trackRef }: { name: string; trackRef: React.Ref<HTMLSpanElement> }) {
  return (
    <span className="min-w-0 flex-1 overflow-hidden">
      <span
        ref={trackRef}
        className="block truncate text-[13px] font-medium text-foreground/90 group-hover:overflow-visible group-hover:text-clip group-hover:text-foreground"
      >
        {name}
      </span>
    </span>
  )
}

/** Status-dot colour: green focused/active, signal on your turn, else muted. Flat if-chain. */
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
  const marquee = useTitleMarquee()

  return (
    <div
      onMouseEnter={marquee.onMouseEnter}
      onMouseLeave={marquee.onMouseLeave}
      className={cn(
        "group relative flex w-full flex-col gap-1 rounded-lg px-2.5 py-2 text-left transition-colors select-none",
        selected ? "card-shadow bg-card" : "hover:card-shadow hover:bg-card",
      )}
    >
      <div {...clickable(() => onSelect(t.id))} className="flex flex-col gap-1 text-left">
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
          <MarqueeTitle name={t.name} trackRef={marquee.trackRef} />
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

/** Hover-revealed row actions: archive/restore, delete (archived), pause/resume (live). Each stops propagation so it doesn't select the row. */
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
        className="flex size-5 items-center justify-center rounded-md text-muted-foreground/60 group-hover:text-foreground hover:bg-muted"
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
          className="flex size-5 items-center justify-center rounded-md text-muted-foreground/60 group-hover:text-foreground hover:bg-muted hover:text-(--danger)"
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
          className="flex size-5 items-center justify-center rounded-md text-muted-foreground/60 group-hover:text-foreground hover:bg-muted"
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
