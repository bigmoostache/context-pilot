import { Archive, ArchiveRestore, ChevronLeft, Pause, Play, Trash2 } from "lucide-react"
import { useRef } from "react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Tip } from "@/components/ui/tip"
import type { ThreadDetail } from "@/lib/types"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { clickable, useLoopNav } from "@/lib/support/a11y"
import {
  byRecent,
  dotColor,
  previewOf,
  ROW_ACTION_COPY,
  threadProgress,
} from "@/lib/support/threadMessages"
import { HintBadge } from "@/components/shell/chrome/HintBadge"
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
  /** Search-palette open state — CONTROLLED (trigger lives in the header rail,
   *  a sibling of this view; the palette renders here for the list + agent). */
  searchOpen: boolean
  onSearchOpenChange: (v: boolean) => void
}

/**
 * Left rail of the thread-centered view — grouped chat sidebar. Threads group by
 * turn-status (**Agent's turn** / **User turn**) with an **Archived** view;
 * search lives in {@link ThreadSearchPalette}; the rail collapses via the header
 * button (T669). Structure (P8): {@link ListHeader} / {@link EmptyState} /
 * {@link RowActions} / {@link RowMeta} keep budgets.
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
  searchOpen,
  onSearchOpenChange,
}: ThreadListProps) {
  const live = threads.filter((t) => !t.archived)
  const archived = threads.filter((t) => t.archived)
  const archivedCount = archived.length

  // the group that's on screen (search now lives in the command palette)
  const visible = showArchived ? archived : live

  /** Sort the "Agent's turn" group focused-first, then by recency (T36) — the
   *  focused thread floats to the top regardless of last-activity time. */
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

  // On-screen order, flattened for ⌘/Ctrl+Up/Down loop nav (T634): live = the
  // two groups top-to-bottom, archived = newest-first. The hook wraps and badges
  // the two rows Up/Down would move to (↑ = prevId, ↓ = nextId) while held.
  const archivedSorted = showArchived ? [...visible].toSorted(byRecent) : []
  const orderedIds = (showArchived ? archivedSorted : [...mine, ...working]).map((t) => t.id)
  const { modHeld: navHeld, prevId, nextId } = useLoopNav(orderedIds, selectedId, onSelect)

  const navHintOf = (id: string): "up" | "down" | undefined =>
    id === prevId ? "up" : id === nextId ? "down" : undefined

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
      navHint={navHintOf(t.id)}
      navHeld={navHeld}
    />
  )

  return (
    // `bg-surface-2`, not `bg-card` (`--card` is the selected row that sits ON
    // this panel). No horizontal margin (`my-2` only): the panel runs flush, the
    // `card-shadow` separates it, and ThreadsView's collapse offset is just
    // `--sidebar-w` with no margins to add on.
    <aside className="card-shadow my-2 flex w-(--sidebar-w) shrink-0 flex-col overflow-hidden rounded-none border border-border bg-surface-2">
      {/* fixed-width inner shell pinned to the rail width */}
      <div
        className="flex h-full flex-col"
        style={{ width: "var(--sidebar-w)", minWidth: "var(--sidebar-w)" }}
      >
        <ListHeader
          showArchived={showArchived}
          onToggleArchived={onToggleArchived}
          archivedCount={archivedCount}
        />

        <ScrollArea className="min-h-0 flex-1">
          {/* The panel's single inner inset — one `p-2` on the content (not the
              ScrollArea) so the scrollbar still runs flush to the panel edge. */}
          <div className="p-2">
            {visible.length === 0 && <EmptyState showArchived={showArchived} />}

            {!showArchived && (
              <>
                {mine.length > 0 && <Group label="Agent's turn" count={mine.length} first />}
                {mine.map((t) => row(t))}

                {working.length > 0 && <Group label="User turn" count={working.length} />}
                {working.map((t) => row(t))}
              </>
            )}

            {showArchived &&
              // Latest-archived first (T277) — most recently active on top.
              archivedSorted.map((t) => row(t, true))}
          </div>
        </ScrollArea>

        {!showArchived && archivedCount > 0 && (
          <button
            onClick={() => onToggleArchived(true)}
            className="hover:card-shadow mx-2 mb-2 flex shrink-0 items-center gap-2 rounded-lg px-3 py-2 text-[12px] text-muted-foreground transition-colors hover:bg-card hover:text-foreground"
          >
            <Archive className="size-3.5" />
            Archived
            <span className="ml-auto text-muted-foreground/50 tabular-nums">{archivedCount}</span>
          </button>
        )}
      </div>

      <ThreadSearchPalette
        open={searchOpen}
        onClose={() => onSearchOpenChange(false)}
        threads={threads}
        agentId={agentId}
        onSelect={(id) => {
          onSelect(id)
          onSearchOpenChange(false)
        }}
      />
    </aside>
  )
}

/** The "‹ Archived" back-link band, shown ONLY while viewing the archived set.
 *  Its former New-thread / Search / collapse cluster moved to the header rail;
 *  on the normal view it now has no content and returns null. */
function ListHeader({
  showArchived,
  onToggleArchived,
  archivedCount,
}: {
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  archivedCount: number
}) {
  if (!showArchived) return null

  return (
    <div className="flex items-center px-3 pt-2">
      <button
        onClick={() => onToggleArchived(false)}
        className="flex items-center gap-1.5 text-[12px] font-medium text-foreground/80 transition-colors hover:text-foreground"
      >
        <ChevronLeft className="size-3.5" />
        Archived
        <span className="text-muted-foreground/50 tabular-nums">{archivedCount}</span>
      </button>
    </div>
  )
}

/** The empty placeholder shown when no thread is visible — copy adapts to which
 *  set (live/archived) is on screen. */
function EmptyState({ showArchived }: { showArchived: boolean }) {
  const message = showArchived ? "No archived threads." : "No threads yet."
  return <p className="px-2.5 py-6 text-center text-[11.5px] text-muted-foreground/55">{message}</p>
}

/** A turn-status band above a run of rows. `pt-3` on every band except the
 *  first (the scroll content's own inset already spaces the first one). */
function Group({ label, count, first }: { label: string; count: number; first?: boolean }) {
  return (
    <div className={cn("flex items-center gap-2 px-2.5 pb-1", first ? "pt-0" : "pt-3")}>
      <span className="text-[11px] font-semibold text-muted-foreground">{label}</span>
      <span className="text-[11px] text-muted-foreground/45 tabular-nums">{count}</span>
    </div>
  )
}

/** Row-hover title marquee (WAA): 0.3s dwell, scroll left at 10 chars/s, 0.3s
 *  dwell, teleport back. No-op if the title fits or prefers-reduced-motion. */
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

function ThreadRow({
  t,
  selected,
  onSelect,
  onArchive,
  onPause,
  onDelete,
  archived,
  navHint,
  navHeld,
}: {
  t: ThreadDetail
  selected: boolean
  onSelect: (id: string) => void
  onArchive: (id: string) => void
  onPause?: ((id: string) => void) | undefined
  onDelete?: ((id: string) => void) | undefined
  archived?: boolean | undefined
  /** Which loop-nav arrow badge this row shows while ⌘/Ctrl is held (T634). */
  navHint?: "up" | "down" | undefined
  /** Whether ⌘/Ctrl is currently held — gates the badge's visibility. */
  navHeld?: boolean | undefined
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
      // `py-1.5` (12px), not `py-2`: two text lines are ~33px. One child, no gap.
      className={cn(
        "group relative mb-0.5 flex w-full flex-col rounded-lg px-2.5 py-1.5 text-left transition-colors select-none",
        selected ? "card-shadow bg-card" : "hover:card-shadow hover:bg-card",
      )}
    >
      {navHint && (
        <HintBadge label={navHint === "up" ? "↑" : "↓"} shown={Boolean(navHeld)} side="left" />
      )}
      {/* `gap-0.5` binds title + preview into ONE unit; rows are kept apart by
          padding + `mb-0.5`, sharpening the hierarchy. */}
      <div {...clickable(() => onSelect(t.id))} className="flex flex-col gap-0.5 text-left">
        {/* line 1 — dot + status badges + name + time + hover actions */}
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
          {isFocused && <StatusBadge tone="ok" label="focused" />}
          {isPaused && <StatusBadge tone="warn" label="paused" />}
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
        {/* line 2 — progress bar (or preview) + unread */}
        <RowMeta t={t} archived={Boolean(archived)} />
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
      <RowAction
        copy={ROW_ACTION_COPY[archived ? "restore" : "archive"]}
        icon={archived ? ArchiveRestore : Archive}
        onClick={() => onArchive(id)}
      />
      {archived && onDelete && (
        <RowAction
          copy={ROW_ACTION_COPY.remove}
          icon={Trash2}
          danger
          onClick={() => onDelete(id)}
        />
      )}
      {!archived && onPause && (
        <RowAction
          copy={ROW_ACTION_COPY[isPaused ? "resume" : "pause"]}
          icon={isPaused ? Play : Pause}
          onClick={() => onPause(id)}
        />
      )}
    </span>
  )
}

/**
 * One hover-revealed row action, with its tooltip. Rest colour is on the BUTTON
 * (`muted-foreground/70`), no hover fill. `danger` is picked by TERNARY (not two
 * `hover:text-*` classes, whose winner would be source-order incidental).
 */
function RowAction({
  copy,
  icon: Icon,
  danger,
  onClick,
}: {
  copy: { readonly title: string; readonly body: string }
  icon: typeof Archive
  danger?: boolean
  onClick: () => void
}) {
  return (
    // `top`: at the rail's right edge, `right` would open over the conversation
    // and `left` back over the rail itself.
    <Tip title={copy.title} body={copy.body} side="top" triggerClassName="inline-flex">
      <button
        onClick={(e) => {
          // Without this, using an action would also select the row behind it.
          e.stopPropagation()
          onClick()
        }}
        aria-label={copy.title}
        className={cn(
          "flex size-5 items-center justify-center rounded-md text-muted-foreground/70 transition-colors",
          danger ? "hover:text-(--danger)" : "hover:text-foreground",
        )}
      >
        <Icon className="size-3" />
      </button>
    </Tip>
  )
}

/** A first-line status pill (focused / paused), tone-keyed to the app palette.
 *  No `rounded-*` — the codebase is square throughout. */
function StatusBadge({ tone, label }: { tone: "ok" | "warn"; label: string }) {
  return (
    <span
      className="shrink-0 px-1.5 py-px text-[9.5px] font-semibold tracking-wide uppercase"
      style={{
        background: `color-mix(in oklab, var(--${tone}) 18%, transparent)`,
        color: `var(--${tone})`,
      }}
    >
      {label}
    </span>
  )
}

/** A row's second line: either the T687 task-progress widget (`x/y` + segmented
 *  bar + label) or the flattened message preview when the thread has no tasks,
 *  and the unread pill. Focused/paused badges now live on the FIRST line. When
 *  every task is done the whole line renders muted so a finished thread reads
 *  calm and doesn't pull focus. */
function RowMeta({ t, archived }: { t: ThreadDetail; archived: boolean }) {
  const progress = threadProgress(t)
  const allDone = progress !== null && progress.done === progress.total
  return (
    <div className="flex items-center gap-1.5 pl-4">
      {progress ? (
        <RowProgress p={progress} muted={allDone} />
      ) : (
        <span className="truncate text-[11.5px] text-muted-foreground/70">{previewOf(t)}</span>
      )}
      {!archived && (t.unread ?? 0) > 0 && (
        <span
          className="ml-auto shrink-0 px-1.5 text-[10px] font-semibold text-(--primary-foreground) tabular-nums"
          style={{ background: "var(--signal)" }}
        >
          {t.unread}
        </span>
      )}
    </div>
  )
}

/**
 * The row's second-line task-progress widget (T687): an `x/y` (done/total)
 * count, then a slim three-segment track — green `done`, orange `inProgress`,
 * gray `planned` (the track showing through) — then the current-front label.
 * When `muted` (the whole thread is done) every part renders in the muted grey
 * so a finished thread doesn't pull focus.
 */
function RowProgress({
  p,
  muted,
}: {
  p: import("@/lib/support/threadMessages").ThreadProgress
  muted: boolean
}) {
  const donePct = p.total ? (p.done / p.total) * 100 : 0
  const inProgPct = p.total ? (p.inProgress / p.total) * 100 : 0
  const countCls = muted ? "text-muted-foreground/60" : "text-muted-foreground/70"
  return (
    <span className="flex min-w-0 flex-1 items-center gap-1.5" title={`${p.done}/${p.total} done`}>
      <span className={"shrink-0 text-[11px] tabular-nums " + countCls}>
        {p.done}/{p.total}
      </span>
      <span className="relative h-1 w-10 shrink-0 overflow-hidden bg-muted">
        {/* done — green, filled from the left */}
        <span
          className="absolute inset-y-0 left-0 transition-[width] duration-300 ease-out"
          style={{ width: `${donePct}%`, background: muted ? "var(--muted-foreground)" : "var(--ok)" }}
        />
        {/* in-progress — orange, starting where done ends (gray track = planned) */}
        <span
          className="absolute inset-y-0 transition-all duration-300 ease-out"
          style={{ left: `${donePct}%`, width: `${inProgPct}%`, background: "var(--warn)" }}
        />
      </span>
      <span className={"truncate text-[11.5px] " + countCls}>{p.label}</span>
    </span>
  )
}
