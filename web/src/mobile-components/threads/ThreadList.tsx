import { useRef, useState } from "react"
import {
  Search,
  X,
  Archive,
  ArchiveRestore,
  ChevronLeft,
  ChevronRight,
  Pause,
  Play,
  Trash2,
  SquarePen,
} from "lucide-react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
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
 * Mobile thread roster — an **iOS-Messages-style** conversation list (T620).
 *
 * The desktop version is a dense sidebar rail (small type, always-visible
 * archive/pause/delete buttons on every row, "Agent's turn"/"User turn" group
 * headers with count pills). None of that reads as native on a phone, so the
 * mobile twin is rebuilt to feel like the iMessage conversation list:
 *
 *   • a large title header with a compose (✎) icon instead of a full-width
 *     button;
 *   • a soft rounded search pill (no heavy bordered card);
 *   • tall rows — an initials avatar, a semibold title with a right-aligned
 *     timestamp, and a two-line grey preview — separated by hairlines inset
 *     past the avatar (not boxed cards);
 *   • a leading accent dot marking a thread that owes you a turn / is unread;
 *   • **swipe-left to reveal** the archive / pause / delete actions, so the
 *     resting row is clean (the desktop's button clutter is gone).
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
      <ListHeader
        showArchived={showArchived}
        onToggleArchived={onToggleArchived}
        liveCount={live.length}
        archivedCount={archivedCount}
        onNewThread={onNewThread}
      />

      {/* search — soft rounded pill, iMessage style */}
      <div className="shrink-0 px-4 pb-2">
        <div className="flex items-center gap-2 rounded-xl bg-muted/60 px-3 py-2 text-[16px]">
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
      </div>

      <ScrollArea className="min-h-0 flex-1">
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
                {/* hairline separator, inset past the avatar */}
                <div className="ml-17 h-px bg-border/60" />
              </li>
            ))}
          </ul>
        )}
      </ScrollArea>

      {/* archived entry point (hidden while already viewing archived) */}
      {!showArchived && archivedCount > 0 && (
        <button
          onClick={() => onToggleArchived(true)}
          className="flex shrink-0 items-center gap-2 border-t border-border px-4 py-3 text-[15px] text-muted-foreground active:bg-muted/50"
        >
          <Archive className="size-4" />
          Archived
          <span className="ml-auto text-muted-foreground/50 tabular-nums">{archivedCount}</span>
          <ChevronRight className="size-4 text-muted-foreground/40" />
        </button>
      )}
    </div>
  )
}

/** Large-title header with a compose action — or, in the archived view, a
 *  back header. iMessage's large-title + top-right compose convention. */
function ListHeader({
  showArchived,
  onToggleArchived,
  liveCount,
  archivedCount,
  onNewThread,
}: {
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  liveCount: number
  archivedCount: number
  onNewThread: () => void
}) {
  if (showArchived) {
    return (
      <div className="flex items-center gap-1 px-3 pt-3 pb-2">
        <button
          onClick={() => onToggleArchived(false)}
          className="flex items-center gap-0.5 text-[16px] text-(--signal) active:opacity-70"
        >
          <ChevronLeft className="size-5" />
          Threads
        </button>
        <span className="ml-2 text-[17px] font-semibold text-foreground">Archived</span>
        <span className="ml-auto pr-1 text-[13px] text-muted-foreground/60 tabular-nums">
          {archivedCount}
        </span>
      </div>
    )
  }
  return (
    <div className="flex items-end justify-between px-4 pt-4 pb-2">
      <div className="flex flex-col">
        <span className="text-[26px] leading-tight font-bold tracking-tight text-foreground">
          Threads
        </span>
        <span className="text-[12px] text-muted-foreground/70 tabular-nums">
          {liveCount} conversation{liveCount === 1 ? "" : "s"}
        </span>
      </div>
      <button
        onClick={onNewThread}
        aria-label="New thread"
        title="New thread"
        className="flex size-9 items-center justify-center rounded-full text-(--signal) active:bg-(--signal)/10"
      >
        <SquarePen className="size-5.5" />
      </button>
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

/** Deterministic avatar tint from the thread id — a stable per-thread colour
 *  so the same conversation always wears the same badge (iMessage-ish). */
const AVATAR_TINTS = [
  "var(--signal)",
  "var(--ok)",
  "var(--warn)",
  "var(--interactive)",
  "var(--danger)",
]
function avatarTint(id: string): string {
  let sum = 0
  for (const ch of id) sum += ch.codePointAt(0) ?? 0
  return AVATAR_TINTS[sum % AVATAR_TINTS.length] ?? "var(--signal)"
}

/** Up-to-two-letter initials from the thread name. */
function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean)
  if (words.length === 0) return "#"
  const first = words[0]?.[0] ?? ""
  const second = words.length > 1 ? (words.at(-1)?.[0] ?? "") : ""
  return (first + second).toUpperCase()
}

/** iMessage-style conversation row: avatar, title + timestamp, 2-line preview,
 *  a leading accent dot when the thread owes you a turn or has unread. The row
 *  content sits above the swipe-revealed action strip (see {@link SwipeRow}). */
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
  const paused = !archived && Boolean(t.paused)
  const unread = !archived && (t.unread ?? 0) > 0
  // The blue "attention" dot: a thread that owes you a reply or carries unread.
  const attention = !archived && (unread || t.status === "MY_TURN" || t.focused)

  return (
    <button
      onClick={() => onSelect(t.id)}
      className={cn(
        "flex w-full items-center gap-3 px-4 py-2.5 text-left",
        selected ? "bg-muted/50" : "bg-background active:bg-muted/40",
      )}
    >
      {/* leading attention dot — occupies a fixed gutter so titles align */}
      <span className="flex w-2 shrink-0 justify-center">
        {attention && <span className="size-2 rounded-full bg-(--signal)" />}
      </span>

      {/* avatar */}
      <span
        className="flex size-11 shrink-0 items-center justify-center rounded-full text-[15px] font-semibold text-white"
        style={{ background: archived ? "var(--muted-foreground)" : avatarTint(t.id) }}
      >
        {initials(t.name)}
      </span>

      {/* text column */}
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-baseline gap-2">
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
        <span className="flex items-center gap-1.5">
          {paused && (
            <span className="shrink-0 text-[11px] font-medium text-(--warn)">Paused ·</span>
          )}
          <span className="line-clamp-2 min-w-0 flex-1 text-[14px] leading-snug text-muted-foreground/70">
            {preview}
          </span>
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
          if (dx === 0) {
          	return;
          }

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
