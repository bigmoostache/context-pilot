import { useRef, useState } from "react"
import {
  Search,
  X,
  Archive,
  ArchiveRestore,
  Pause,
  Play,
  Trash2,
  SquarePen,
  ChevronRight,
} from "lucide-react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import { CornerButton } from "@/mobile-components/shell/CornerButton"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"
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

/** Sort by most recent activity first. */
function byRecent(a: ThreadDetail, b: ThreadDetail): number {
  return (b.lastActivityMs ?? 0) - (a.lastActivityMs ?? 0)
}

/** Focused first, then agent-owes-you (MY_TURN), then most-recent — so the
 *  thread that wants attention floats to the top of the flat list. */
function byPriority(a: ThreadDetail, b: ThreadDetail): number {
  const rank = (t: ThreadDetail) => (t.focused ? 0 : t.status === "MY_TURN" ? 1 : 2)
  const ra = rank(a)
  const rb = rank(b)
  return ra === rb ? byRecent(a, b) : ra - rb
}

/**
 * Mobile thread roster — an **iOS-Messages-style** conversation list (T620/T625).
 *
 * The desktop version is a dense sidebar rail. The mobile twin is rebuilt to feel
 * native, and (T625) to put every interactive control where a thumb can reach it:
 *
 *   • **No top title bar.** The desktop "Threads / N conversations" header + a
 *     top compose button are gone — a phone shouldn't hang primary actions off
 *     the top edge (hard to reach, and under the status bar in standalone).
 *   • **Bottom action bar.** Search moves to the BOTTOM of the screen with the
 *     compose (new-thread) button beside it — both within thumb reach, safe-area
 *     padded so they clear the home indicator.
 *   • **Archived toggle in the top-right corner** — a shared {@link CornerButton}
 *     (safe-area-offset so it's always tappable, even in standalone), flipping
 *     between the live and archived sets. This replaces the old bottom
 *     "Archived (N) ›" entry row and the archived back-header.
 *   • tall rows (avatar, title + timestamp, 2-line preview), hairline
 *     separators, a leading accent dot for a thread that owes you a turn, and
 *     **swipe-left** to reveal archive / pause / delete.
 *
 * All filtering / search / sort logic is shared with desktop — only the chrome
 * and the touch affordances fork.
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

  const source = showArchived ? archived : live
  const visible = source.filter((t) => matches(t)).toSorted(showArchived ? byRecent : byPriority)

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden bg-background">
      {/* Top-right corner control: flip between live and archived. Shares the
          safe-area-aware CornerButton so it's reachable even in standalone.
          Hidden when there's nothing archived AND we're in the live view (no
          set to switch to). */}
      {(showArchived || archivedCount > 0) && (
        <CornerButton
          side="right"
          label={showArchived ? "Show active threads" : "Show archived threads"}
          onClick={() => onToggleArchived(!showArchived)}
          className="z-30"
        >
          {showArchived ? (
            <ArchiveRestore className="size-4.5 text-(--signal)" />
          ) : (
            <Archive className="size-4.5" />
          )}
        </CornerButton>
      )}

      {/* A minimal caption ONLY in the archived view, so the mode is legible
          (the live view is deliberately chrome-free). */}
      {showArchived && (
        <div className="shrink-0 px-4 pt-[calc(env(safe-area-inset-top)+0.75rem)] pb-1 text-center">
          <span className="text-[13px] font-semibold tracking-wide text-muted-foreground/70">
            Archived
          </span>
        </div>
      )}

      <ScrollArea className="min-h-0 flex-1">
        {/* pad the top so the first row clears the floating corner button in the
            live (caption-less) view */}
        <div className={cn(showArchived ? "" : "pt-[calc(env(safe-area-inset-top)+3rem)]")}>
          {visible.length === 0 ? (
            <EmptyState hasQuery={q !== ""} showArchived={showArchived} />
          ) : (
            <ul className="flex flex-col">
              {visible.map((t) => (
                <li key={t.id}>
                  <SwipeRow
                    archived={showArchived}
                    isPaused={!showArchived && Boolean(t.paused)}
                    onArchive={() => onArchive(t.id)}
                    onDelete={() => onDelete(t.id)}
                    onPause={() => onPause(t.id)}
                  >
                    <ThreadRow
                      t={t}
                      selected={t.id === selectedId}
                      archived={showArchived}
                      onSelect={onSelect}
                    />
                  </SwipeRow>
                </li>
              ))}
            </ul>
          )}
        </div>
      </ScrollArea>

      {/* Bottom action bar — search + compose, both in thumb reach (T625). */}
      <div className="flex shrink-0 items-center gap-2 border-t border-border/70 px-3 pt-2 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
        <div className="flex flex-1 items-center gap-2 rounded-xl bg-muted/60 px-3 py-2 text-[16px]">
          <Search className="size-4 shrink-0 text-muted-foreground/60" />
          <input
            value={query}
            onChange={(e) => onQueryChange(e.target.value)}
            placeholder={showArchived ? "Search archived" : "Search"}
            className="min-w-0 flex-1 bg-transparent text-foreground/90 outline-none placeholder:text-muted-foreground/55"
          />
          {query && (
            <button
              onClick={() => onQueryChange("")}
              className="shrink-0 text-muted-foreground/55 active:text-foreground"
              title="Clear"
            >
              <X className="size-4" />
            </button>
          )}
        </div>
        {/* compose — the primary create affordance, a filled circle at the
            bottom-right corner of the bar (hidden in the archived view, where
            creating a thread makes no sense) */}
        {!showArchived && (
          <button
            onClick={onNewThread}
            aria-label="New thread"
            className="flex size-11 shrink-0 items-center justify-center rounded-full bg-(--signal) text-(--primary-foreground) transition-[filter] active:brightness-110"
          >
            <SquarePen className="size-5" />
          </button>
        )}
      </div>
    </div>
  )
}

/** The empty placeholder — copy adapts to search / which set is shown. */
function EmptyState({ hasQuery, showArchived }: { hasQuery: boolean; showArchived: boolean }) {
  const message = hasQuery
    ? "No threads match your search."
    : showArchived
      ? "No archived threads."
      : "No conversations yet."
  return (
    <p className="px-4 py-16 text-center text-[14px] text-muted-foreground/55">{message}</p>
  )
}

// ── row ──────────────────────────────────────────────────────────────

/**
 * The leading status dot colour, encoding whose turn it is / what the thread is
 * doing (T627). One dot per row, first match wins:
 *   • yellow  (--warn)  — paused: the agent won't act until resumed;
 *   • green   (--ok)    — focused: the agent is actively on this thread now;
 *   • orange  (--signal)— agent's turn (MY_TURN / ACTIVE) but not yet focused —
 *                         it will pick this up soon;
 *   • grey    (muted)   — the user's turn (THEIR_TURN), nothing pending on the
 *                         agent side. Archived rows are always grey (inactive).
 */
function statusTint(t: ThreadDetail, archived: boolean): string {
  if (archived) return "var(--muted-foreground)"
  if (t.paused) return "var(--warn)"
  if (t.focused) return "var(--ok)"
  if (t.status === "MY_TURN" || t.status === "ACTIVE") return "var(--signal)"
  return "var(--muted-foreground)"
}

/** iMessage-style conversation row: a leading status dot, title + timestamp, a
 *  2-line preview. The letter-avatar badge is gone (T627 — the initials carried
 *  no meaning); the coloured dot before the title now conveys the thread state.
 *  The row content sits above the swipe-revealed action strip ({@link SwipeRow}). */
function ThreadRow({
  t,
  selected,
  archived,
  onSelect,
}: {
  t: ThreadDetail
  selected: boolean
  archived: boolean
  onSelect: (id: string) => void
}) {
  const preview = previewOf(t)
  // Bold the title when the agent owes this thread a turn (or it's focused /
  // unread) — a subtle emphasis on the rows that want attention.
  const attention =
    !archived && ((t.unread ?? 0) > 0 || t.status === "MY_TURN" || t.focused)

  return (
    <button
      onClick={() => onSelect(t.id)}
      className={cn(
        "flex w-full items-center gap-3 px-4 py-2.5 text-left",
        selected ? "bg-muted/50" : "bg-background active:bg-muted/40",
      )}
    >
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-baseline gap-2">
          {/* status dot — inline before the title, coloured by thread state */}
          <span
            className="size-2.5 shrink-0 translate-y-px rounded-full"
            style={{ background: statusTint(t, archived) }}
          />
          <span
            className={cn(
              "truncate text-[16px] text-foreground",
              attention ? "font-semibold" : "font-medium",
            )}
          >
            {t.name}
          </span>
          <span className="ml-auto shrink-0 text-[12px] text-muted-foreground/55 tabular-nums">
            {t.lastActivity}
          </span>
          <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/30" />
        </span>
        <span className="line-clamp-2 min-w-0 text-[14px] leading-snug text-muted-foreground/70">
          {preview}
        </span>
      </span>
    </button>
  )
}

// ── swipe-to-reveal actions ──────────────────────────────────────────

/** Pixel width of the revealed action strip (two 68px action buttons). */
const ACTION_W = 136

/**
 * Wrap a row so a **left-swipe** slides it aside to reveal its trailing
 * actions (archive / pause / delete) — the native iOS conversation-list
 * gesture, replacing the desktop's always-visible buttons. The row content
 * translates on X; the action strip sits pinned behind its right edge. A
 * partial swipe snaps open/closed on release, and tapping an open row closes
 * it (so a mis-swipe never eats the next tap).
 */
function SwipeRow({
  children,
  archived,
  isPaused,
  onArchive,
  onDelete,
  onPause,
}: {
  children: React.ReactNode
  archived: boolean
  isPaused: boolean
  onArchive: () => void
  onDelete: () => void
  onPause: () => void
}) {
  const [dx, setDx] = useState(0)
  const startXRef = useRef(0)
  const baseXRef = useRef(0)
  const open = dx <= -ACTION_W / 2

  const onTouchStart = (e: React.TouchEvent) => {
    startXRef.current = e.touches[0]?.clientX ?? 0
    baseXRef.current = dx
  }
  const onTouchMove = (e: React.TouchEvent) => {
    const cur = e.touches[0]?.clientX ?? 0
    const next = Math.min(0, Math.max(-ACTION_W, baseXRef.current + (cur - startXRef.current)))
    setDx(next)
  }
  const onTouchEnd = () => setDx(open ? -ACTION_W : 0)

  // Second action is delete for an archived row (permanent), else pause/resume.
  return (
    <div className="relative overflow-hidden">
      {/* action strip — behind the row content, pinned to the right edge */}
      <div className="absolute inset-y-0 right-0 flex" style={{ width: ACTION_W }}>
        <button
          onClick={() => {
            onArchive()
            setDx(0)
          }}
          className="flex w-1/2 flex-col items-center justify-center gap-0.5 bg-(--warn) text-[11px] font-medium text-white"
        >
          {archived ? <ArchiveRestore className="size-4" /> : <Archive className="size-4" />}
          {archived ? "Restore" : "Archive"}
        </button>
        {archived ? (
          <button
            onClick={() => {
              onDelete()
              setDx(0)
            }}
            className="flex w-1/2 flex-col items-center justify-center gap-0.5 bg-(--danger) text-[11px] font-medium text-white"
          >
            <Trash2 className="size-4" />
            Delete
          </button>
        ) : (
          <button
            onClick={() => {
              onPause()
              setDx(0)
            }}
            className="flex w-1/2 flex-col items-center justify-center gap-0.5 bg-(--interactive) text-[11px] font-medium text-white"
          >
            {isPaused ? <Play className="size-4" /> : <Pause className="size-4" />}
            {isPaused ? "Resume" : "Pause"}
          </button>
        )}
      </div>

      {/* row content — slides left on swipe; tapping while open just closes */}
      <div
        onTouchStart={onTouchStart}
        onTouchMove={onTouchMove}
        onTouchEnd={onTouchEnd}
        onClickCapture={(e) => {
          if (dx === 0) return
          e.preventDefault()
          e.stopPropagation()
          setDx(0)
        }}
        className="relative bg-background transition-transform duration-150"
        style={{ transform: `translateX(${dx}px)` }}
      >
        {children}
      </div>
    </div>
  )
}
