import { useEffect, useRef } from "react"
import { animate, stagger } from "animejs"
import {
  Search,
  X,
  Archive,
  ArchiveRestore,
  Pause,
  Play,
  Trash2,
  Plus,
  ChevronRight,
  LayoutGrid,
} from "lucide-react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import { CornerButton } from "@/mobile-components/shell/CornerButton"
import { FrostedBottomBar } from "@/mobile-components/shell/FrostedBottomBar"
import { useElementHeight } from "@/lib/live/useElementHeight"
import type { ThreadDetail } from "@/lib/types"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { previewOf } from "@/lib/support/threadMessages"
import { useSwipeRow } from "@/lib/live/useSwipeRow"

interface ThreadListProps {
  /** all of the realm's threads (archived included) — filtering happens here */
  threads: ThreadDetail[]
  selectedId: string
  onSelect: (id: string) => void
  /** leave for the agents (fleet) page — the sidebar's top-left corner button (T631) */
  onGoToAgents?: (() => void) | undefined
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
  /** create a thread named `name` — the bottom search field doubles as the
   *  create input, so its current value IS the new thread's name (T633) */
  onNewThread: (name: string) => void
}

/** Sort by most recent activity first. */
function byRecent(a: ThreadDetail, b: ThreadDetail): number {
  return (b.lastActivityMs ?? 0) - (a.lastActivityMs ?? 0)
}

/** Focused first, then MY_TURN (agent owes you), then most-recent. */
function byPriority(a: ThreadDetail, b: ThreadDetail): number {
  const rank = (t: ThreadDetail) => (t.focused ? 0 : t.status === "MY_TURN" ? 1 : 2)
  const ra = rank(a)
  const rb = rank(b)
  return ra === rb ? byRecent(a, b) : ra - rb
}

/**
 * Mobile thread roster — an **iOS-Messages-style** conversation list (T620/T625).
 *
 * Rebuilt from the dense desktop sidebar to feel native and put every control in
 * thumb reach: no top title bar; search + compose live in a BOTTOM action bar
 * (safe-area padded); the archived toggle is a top-right {@link CornerButton};
 * tall rows (leading status dot, title + timestamp, 2-line preview) with hairline
 * separators; **swipe-left** reveals archive / pause / delete. All filtering /
 * search / sort logic is shared with desktop — only the chrome + touch
 * affordances fork.
 */
export function ThreadList({
  threads,
  selectedId,
  onSelect,
  onGoToAgents,
  query,
  onQueryChange,
  showArchived,
  onToggleArchived,
  onArchive,
  onDelete,
  onPause,
  onNewThread,
}: ThreadListProps) {
  // The bottom search/create bar floats as a glass overlay (T637), so reserve a
  // 1.5× bottom spacer in the scroll content sized from its measured height —
  // the last row must scroll clear of the frosted bar. Mirrors the composer.
  const barRef = useRef<HTMLDivElement>(null)
  const barH = useElementHeight(barRef)

  const q = query.trim().toLowerCase()
  const matches = (t: ThreadDetail) =>
    q === "" || t.name.toLowerCase().includes(q) || previewOf(t).toLowerCase().includes(q)

  const live = threads.filter((t) => !t.archived)
  const archived = threads.filter((t) => t.archived)
  const archivedCount = archived.length

  const source = showArchived ? archived : live
  const visible = source.filter((t) => matches(t)).toSorted(showArchived ? byRecent : byPriority)

  // #3 List cascade (anime.js): stagger the rows in on first mount and whenever
  // the set flips live↔archived. Deliberately keyed ONLY on `showArchived` (not
  // the search query) — re-running the reveal on every keystroke would flicker
  // the filtered list; search stays instant. Honours prefers-reduced-motion.
  const listRef = useRef<HTMLUListElement>(null)
  useEffect(() => {
    const ul = listRef.current
    if (!ul || prefersReducedMotion()) return
    animate(ul.children, {
      opacity: [0, 1],
      translateY: [6, 0],
      delay: stagger(18),
      duration: 300,
      ease: "out(2)",
    })
  }, [showArchived])

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-background">
      {/* Top-left corner control: leave the thread sidebar for the agents (fleet)
          page (T631). Shares the safe-area-aware CornerButton so it's reachable
          even in standalone; the archived toggle mirrors it on the right. */}
      {onGoToAgents && (
        <CornerButton side="left" label="Show agents" onClick={onGoToAgents} className="z-30">
          <LayoutGrid />
        </CornerButton>
      )}

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
            <ArchiveRestore className="text-(--signal)" />
          ) : (
            <Archive />
          )}
        </CornerButton>
      )}

      {/* No "Archived" caption: it sat OUTSIDE the ScrollArea as a non-scrolling
          strip that occupied the safe-area region, so archived content could
          never scroll UNDER the iOS status bar (T639). The mode is already
          conveyed by the top-right toggle glyph (ArchiveRestore, signal-tinted),
          so the caption was redundant chrome — dropped, and the archived list
          now pads + scrolls edge-to-edge exactly like the live list. */}
      <ScrollArea className="min-h-0 flex-1">
        {/* pad the top so the first row clears the floating corner buttons AND
            sits below the iOS status bar at rest, while scrolling edge-to-edge
            UNDER it — applied in BOTH live and archived modes now (T639). */}
        <div className="pt-[calc(env(safe-area-inset-top)+3rem)]">
          {visible.length === 0 ? (
            <EmptyState hasQuery={q !== ""} showArchived={showArchived} />
          ) : (
            <ul ref={listRef} className="flex flex-col">
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
          {/* Bottom spacer = 1.5× the floating bar height, so the last row can
              always scroll clear of the frosted glass bar (T637). */}
          <div aria-hidden style={{ height: barH * 1.5 }} />
        </div>
      </ScrollArea>

      {/* Bottom action bar — the search field DOUBLES as the create input
          (T633): typing filters the list live AND is the draft name for a new
          thread. A create button appears only once the field is non-empty (live
          view); tapping it — or pressing return — creates a thread with that
          name and clears the field. */}
      <FrostedBottomBar
        ref={barRef}
        className="flex items-center gap-2 px-3 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]"
      >
        <div className="flex flex-1 items-center gap-2 rounded-xl bg-muted/60 px-3 py-2 text-[16px]">
          <Search className="size-4 shrink-0 text-muted-foreground/60" />
          <input
            value={query}
            onChange={(e) => onQueryChange(e.target.value)}
            onKeyDown={(e) => {
              // Return creates a thread named after the current value (live view
              // only) — the mobile keyboard's "go" affordance for the dual-use
              // field. No-op on an empty field or in the archived view.
              if (e.key !== "Enter" || showArchived || query.trim() === "") return
              e.preventDefault()
              onNewThread(query.trim())
            }}
            placeholder={showArchived ? "Search archived" : "Search or create a thread"}
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
        {/* Create — appears only when the field is non-empty (live view): the
            field's value is the new thread's name. Hidden in the archived view
            (creating there makes no sense) and when the field is empty (nothing
            to name). */}
        {!showArchived && query.trim() !== "" && (
          <button
            onClick={() => onNewThread(query.trim())}
            aria-label="Create thread"
            className="flex size-11 shrink-0 items-center justify-center rounded-full bg-(--signal) text-(--primary-foreground) transition-[filter] active:brightness-110"
          >
            <Plus className="size-5" />
          </button>
        )}
      </FrostedBottomBar>
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
 * The leading status dot colour, encoding whose turn it is (T627). First match
 * wins: yellow (--warn) paused; green (--ok) focused; orange (--signal) agent's
 * turn (MY_TURN / ACTIVE) not yet focused; grey (muted) the user's turn, or any
 * archived row (inactive).
 */
function statusTint(t: ThreadDetail, archived: boolean): string {
  if (archived) return "var(--muted-foreground)"
  if (t.paused) return "var(--warn)"
  if (t.focused) return "var(--ok)"
  if (t.status === "MY_TURN" || t.status === "ACTIVE") return "var(--signal)"
  return "var(--muted-foreground)"
}

/** iMessage-style conversation row: leading status dot, title + timestamp, a
 *  2-line preview. The coloured dot before the title conveys thread state (T627).
 *  Row content sits above the swipe-revealed action strip ({@link SwipeRow}). */
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
 * Wrap a row so a **left-swipe** slides it aside to reveal its trailing actions
 * (archive / pause / delete) — the native iOS conversation-list gesture. All the
 * gesture feel (direct-DOM-write drag, axis lock, pointer capture, velocity
 * flick, spring snap) lives in {@link useSwipeRow}; this component is just the
 * action strip + the bound sliding row.
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
  const { rowRef, close, bind } = useSwipeRow(ACTION_W)

  // Second action is delete for an archived row (permanent), else pause/resume.
  return (
    <div className="relative overflow-hidden">
      {/* action strip — behind the row content, pinned to the right edge */}
      <div className="absolute inset-y-0 right-0 flex" style={{ width: ACTION_W }}>
        <button
          onClick={() => {
            onArchive()
            close()
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
              close()
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
              close()
            }}
            className="flex w-1/2 flex-col items-center justify-center gap-0.5 bg-(--interactive) text-[11px] font-medium text-white"
          >
            {isPaused ? <Play className="size-4" /> : <Pause className="size-4" />}
            {isPaused ? "Resume" : "Pause"}
          </button>
        )}
      </div>

      {/* Row content — slides left on swipe. `touch-pan-y` keeps native vertical
          scroll while the hook owns the horizontal drag; the transform is written
          directly to this node (never via a React `style` prop) so a drag causes
          zero re-renders. Tapping while open just closes. */}
      <div ref={rowRef} {...bind} className="relative touch-pan-y bg-background select-none">
        {children}
      </div>
    </div>
  )
}
