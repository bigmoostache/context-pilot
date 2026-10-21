import { useEffect, useRef, useState } from "react"
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
  Pencil,
  LayoutGrid,
} from "lucide-react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import { CornerButton } from "@/mobile-components/shell/chrome/CornerButton"
import { FrostedBottomBar } from "@/mobile-components/shell/chrome/FrostedBottomBar"
import { useElementHeight } from "@/lib/live/useElementHeight"
import type { AgentDetail } from "@/lib/types"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { litColor, previewOf, treeGuides, type TreeGuide } from "@/lib/support/agentMessages"
import { PendingSpinner } from "@/mobile-components/ui/pending"
import { RenameInput } from "./dialogs/RenameInput"
import { useSwipeRow } from "@/lib/live/useSwipeRow"

interface AgentListProps {
  /** all of the realm's agents (archived included) — filtering happens here */
  agents: AgentDetail[]
  selectedId: string
  onSelect: (id: string) => void
  /** leave for the agents (fleet) page — the sidebar's top-left corner button (T631) */
  onGoToFleets?: (() => void) | undefined
  /** live search query (controlled by the parent so it survives navigation) */
  query: string
  onQueryChange: (q: string) => void
  /** archived view toggle */
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  /** archive ↔ restore a single agent */
  onArchive: (id: string) => void
  /** permanently delete a agent (T371) */
  onDelete: (id: string) => void
  /** pause ↔ resume a single agent (T371) */
  onPause: (id: string) => void
  /** rename a live agent */
  onRename: (id: string, name: string) => void
  /** create a agent named `name` — the bottom search field doubles as the
   *  create input, so its current value IS the new agent's name (T633) */
  onNewAgent: (name: string) => void
}

/**
 * Mobile agent roster — an **iOS-Messages-style** conversation list (T620/T625).
 *
 * Rebuilt from the dense desktop sidebar to feel native and put every control in
 * thumb reach: no top title bar; search + compose live in a BOTTOM action bar
 * (safe-area padded); the archived toggle is a top-right {@link CornerButton};
 * one-line rows (status dot, title, unread badge) indented as a tree by
 * `depth`; **swipe-left** reveals archive / pause / delete. All filtering /
 * search / sort logic is shared with desktop — only the chrome + touch
 * affordances fork.
 */
export function AgentList({
  agents,
  selectedId,
  onSelect,
  onGoToFleets,
  query,
  onQueryChange,
  showArchived,
  onToggleArchived,
  onArchive,
  onDelete,
  onPause,
  onRename,
  onNewAgent,
}: AgentListProps) {
  const [renamingId, setRenamingId] = useState<string | null>(null) // swipe → Rename row
  // The bottom bar floats as glass (T637): a 1.5× spacer sized from its measured
  // height lets the last row scroll clear of it. Mirrors the composer.
  const barRef = useRef<HTMLDivElement>(null)
  const barH = useElementHeight(barRef)

  const q = query.trim().toLowerCase()
  const matches = (t: AgentDetail) =>
    q === "" || t.name.toLowerCase().includes(q) || previewOf(t).toLowerCase().includes(q)

  const live = agents.filter((t) => !t.archived)
  const archived = agents.filter((t) => t.archived)
  const archivedCount = archived.length

  const source = showArchived ? archived : live
  // Backend order = sidebar tree (creation order, sub-agents under their
  // parent, `depth` per row, T860); render as received.
  const visible = source.filter((t) => matches(t))
  const guides = treeGuides(visible)

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
      {/* Top-left corner control: leave the agent sidebar for the agents (fleet)
          page (T631). Shares the safe-area-aware CornerButton so it's reachable
          even in standalone; the archived toggle mirrors it on the right. */}
      {onGoToFleets && (
        <CornerButton side="left" label="Show fleets" onClick={onGoToFleets} className="z-30">
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
          label={showArchived ? "Show active agents" : "Show archived agents"}
          onClick={() => onToggleArchived(!showArchived)}
          className="z-30"
        >
          {showArchived ? <ArchiveRestore className="text-(--signal)" /> : <Archive />}
        </CornerButton>
      )}

      {/* No "Archived" caption: it sat OUTSIDE the ScrollArea as a non-scrolling
          strip that occupied the safe-area region, so archived content could
          never scroll UNDER the iOS status bar (T639). The mode is already
          conveyed by the top-right toggle glyph (ArchiveRestore, signal-tinted),
          so the caption was redundant chrome — dropped, and the archived list
          now pads + scrolls edge-to-edge exactly like the live list. */}
      <ScrollArea className="min-h-0 flex-1">
        {/* top pad clears the corner buttons + iOS status bar at rest; content
            still scrolls edge-to-edge UNDER it, live and archived (T639). */}
        <div className="pt-[calc(env(safe-area-inset-top)+3rem)]">
          {visible.length === 0 ? (
            <EmptyState hasQuery={q !== ""} showArchived={showArchived} />
          ) : (
            <ul ref={listRef} className="flex flex-col">
              {visible.map((t) => (
                <li key={t.id}>
                  {renamingId === t.id ? (
                    <RenameRow t={t} onRename={onRename} onDone={() => setRenamingId(null)} />
                  ) : (
                    <SwipeRow
                      archived={showArchived}
                      isPaused={!showArchived && Boolean(t.paused)}
                      onArchive={() => onArchive(t.id)}
                      onDelete={() => onDelete(t.id)}
                      onPause={() => onPause(t.id)}
                      onRename={() => setRenamingId(t.id)}
                    >
                      <AgentRow
                        t={t}
                        selected={t.id === selectedId}
                        archived={showArchived}
                        onSelect={(id) => {
                          if (!t.pending) onSelect(id)
                        }}
                        guide={guides.get(t.id)}
                      />
                    </SwipeRow>
                  )}
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
          agent. A create button appears only once the field is non-empty (live
          view); tapping it — or pressing return — creates a agent with that
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
              // Return creates a agent named after the current value (live view
              // only) — the mobile keyboard's "go" affordance for the dual-use
              // field. No-op on an empty field or in the archived view.
              if (e.key !== "Enter" || showArchived || query.trim() === "") return
              e.preventDefault()
              onNewAgent(query.trim())
            }}
            placeholder={showArchived ? "Search archived" : "Search or create a agent"}
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
            field's value is the new agent's name. Hidden in the archived view
            (creating there makes no sense) and when the field is empty (nothing
            to name). */}
        {!showArchived && query.trim() !== "" && (
          <button
            onClick={() => onNewAgent(query.trim())}
            aria-label="Create agent"
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
    ? "No agents match your search."
    : showArchived
      ? "No archived agents."
      : "No conversations yet."
  return <p className="px-4 py-16 text-center text-[14px] text-muted-foreground/55">{message}</p>
}

// ── row ──────────────────────────────────────────────────────────────

/** One-line conversation row: tree indent by `depth` (guide line per level),
 *  status dot, title, unread badge at the end. Sits above the swipe-revealed
 *  action strip ({@link SwipeRow}). */
function AgentRow({
  t,
  selected,
  archived,
  onSelect,
  guide,
}: {
  t: AgentDetail
  selected: boolean
  archived: boolean
  onSelect: (id: string) => void
  guide?: TreeGuide | undefined
}) {
  const unread = archived ? 0 : (t.unread ?? 0)
  const attention = !archived && (unread > 0 || t.status === "MY_TURN" || Boolean(t.focused))
  return (
    <button
      onClick={() => onSelect(t.id)}
      className={cn(
        "relative flex w-full items-stretch px-4 text-left",
        selected ? "bg-muted/50" : "bg-background active:bg-muted/40",
      )}
    >
      <TreeGuides depth={t.depth ?? 0} guide={guide} />
      <span className="flex min-w-0 flex-1 items-center gap-2 py-3">
        <span className="size-2.5 shrink-0" style={{ background: litColor(t, archived) }} />
        <span
          className={cn(
            "min-w-0 flex-1 truncate text-[16px] text-foreground",
            attention ? "font-semibold" : "font-medium",
          )}
        >
          {t.name}
        </span>
        {t.pending && <PendingSpinner />}
        {unread > 0 && (
          <span
            className="shrink-0 px-1.5 text-[12px] font-semibold text-(--primary-foreground) tabular-nums"
            style={{ background: "var(--signal)" }}
          >
            {unread}
          </span>
        )}
        <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/30" />
      </span>
    </button>
  )
}

/** Tree indent: one 16px column per depth level. Ancestor columns carry a line
 *  only while their branch continues below; the own column draws the upper half
 *  + elbow, plus the lower half when a sibling follows. A parent gets a stub from
 *  its dot down to its children's column. */
function TreeGuides({ depth, guide }: { depth: number; guide: TreeGuide | undefined }) {
  const line = "absolute left-1.5 w-px bg-muted-foreground/25"
  return (
    <>
      {depth > 0 && (
        <span aria-hidden className="flex shrink-0">
          {Array.from({ length: depth }, (_, i) => {
            const own = i === depth - 1
            return (
              <span key={i} className="relative w-4">
                {own ? (
                  <>
                    <span className={cn(line, "top-0", guide?.more ? "bottom-0" : "h-1/2")} />
                    <span className="absolute top-1/2 left-1.5 h-px w-2.5 bg-muted-foreground/25" />
                  </>
                ) : (
                  guide?.through[i] && <span className={cn(line, "inset-y-0")} />
                )}
              </span>
            )
          })}
        </span>
      )}
      {guide?.kids && (
        <span
          aria-hidden
          className="absolute top-1/2 bottom-0 w-px bg-muted-foreground/25"
          style={{ left: 22 + depth * 16 }}
        />
      )}
    </>
  )
}

// ── swipe-to-reveal actions ──────────────────────────────────────────

/** A row swapped for the inline name editor (swipe → Rename); `onDone` always fires. */
function RenameRow({
  t,
  onRename,
  onDone,
}: {
  t: AgentDetail
  onRename: (id: string, name: string) => void
  onDone: () => void
}) {
  return (
    <div className="flex items-center px-4 py-2.5">
      <RenameInput
        name={t.name}
        className="py-1 text-[16px]"
        onCommit={(next) => {
          onDone()
          onRename(t.id, next)
        }}
        onCancel={onDone}
      />
    </div>
  )
}

/** Width of one 68px swipe action button; live rows show 3, archived rows 2. */
const ACTION_BTN_W = 68

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
  onRename,
}: {
  children: React.ReactNode
  archived: boolean
  isPaused: boolean
  onArchive: () => void
  onDelete: () => void
  onPause: () => void
  onRename: () => void
}) {
  const actionW = ACTION_BTN_W * (archived ? 2 : 3)
  const { rowRef, close, bind } = useSwipeRow(actionW)

  // Live row: rename / archive / pause. Archived row: restore / delete.
  return (
    <div className="relative overflow-hidden">
      {/* action strip — behind the row content, pinned to the right edge */}
      <div className="absolute inset-y-0 right-0 flex" style={{ width: actionW }}>
        {!archived && (
          <button
            onClick={() => {
              close()
              onRename()
            }}
            className="flex flex-1 flex-col items-center justify-center gap-0.5 bg-muted-foreground text-[11px] font-medium text-white"
          >
            <Pencil className="size-4" />
            Rename
          </button>
        )}
        <button
          onClick={() => {
            onArchive()
            close()
          }}
          className="flex flex-1 flex-col items-center justify-center gap-0.5 bg-(--warn) text-[11px] font-medium text-white"
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
            className="flex flex-1 flex-col items-center justify-center gap-0.5 bg-(--danger) text-[11px] font-medium text-white"
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
            className="flex flex-1 flex-col items-center justify-center gap-0.5 bg-(--interactive) text-[11px] font-medium text-white"
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
