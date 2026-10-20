import { Archive, ArchiveRestore, ChevronLeft, Pause, Pencil, Play, Trash2 } from "lucide-react"
import { useRef, useState } from "react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { PendingSpinner } from "@/components/ui/pending"
import { SelectionGlide } from "@/components/ui/selectionGlide"
import { useDepartures, useTitleMarquee } from "@/components/ui/rowMotion"
import { Tip } from "@/components/ui/tip"
import type { AgentDetail } from "@/lib/types"
import { cn } from "@/lib/utils"
import { clickable, useLoopNav } from "@/lib/support/a11y"
import { litColor, ROW_ACTION_COPY, treeGuides, type TreeGuide } from "@/lib/support/agentMessages"
import { HintBadge } from "@/components/shell/chrome/HintBadge"
import { useAgentReadState } from "@/lib/live/agentView"
import { AgentSearchPalette } from "./dialogs/AgentSearchPalette"
import { RenameInput } from "./dialogs/RenameInput"

interface AgentListProps {
  /** all of the realm's agents (archived included) — filtering happens here */
  agents: AgentDetail[]
  /** owning agent — the conversation-search target for the palette (T671) */
  fleetId: string
  selectedId: string
  onSelect: (id: string) => void
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  onArchive: (id: string) => void
  /** permanently delete a agent (T371) */
  onDelete: (id: string) => void
  /** pause ↔ resume a single agent (T371) */
  onPause: (id: string) => void
  /** rename a live agent */
  onRename: (id: string, name: string) => void
  /** Search-palette open state — CONTROLLED (trigger lives in the header rail,
   *  a sibling of this view; the palette renders here for the list + agent). */
  searchOpen: boolean
  onSearchOpenChange: (v: boolean) => void
}

/**
 * Left rail of the agent-centered view: one line per agent, as a tree in the
 * order the backend sends (creation order, sub-agents under their parent, T860),
 * with an **Archived** view; search lives in {@link AgentSearchPalette}; the
 * rail collapses via the header button (T669).
 */
export function AgentList({
  agents: rawAgents,
  fleetId,
  selectedId,
  onSelect,
  showArchived,
  onToggleArchived,
  onArchive,
  onDelete,
  onPause,
  onRename,
  searchOpen,
  onSearchOpenChange,
}: AgentListProps) {
  // Overlay each agent's `unread` with the client-side "not read by the user"
  // count (T712) — the badge now reflects the human's read receipts, not the
  // agent's. Opening a agent marks it read as a side effect of this hook.
  const { unreadOf } = useAgentReadState(fleetId, rawAgents, selectedId)
  const agents = rawAgents.map((t) => ({ ...t, unread: unreadOf(t) }))
  const live = agents.filter((t) => !t.archived)
  const archived = agents.filter((t) => t.archived)
  const archivedCount = archived.length

  // The backend sends the list in sidebar-tree order (creation order,
  // sub-agents right under their parent, `depth` per row); render as received.
  const visible = showArchived ? archived : live

  // On-screen order for ⌘/Ctrl+Up/Down loop nav (T634). The hook wraps and
  // badges the two rows Up/Down would move to (↑ = prevId, ↓ = nextId).
  const orderedIds = visible.map((t) => t.id)
  const { modHeld: navHeld, prevId, nextId } = useLoopNav(orderedIds, selectedId, onSelect)

  const navHintOf = (id: string): "up" | "down" | undefined =>
    id === prevId ? "up" : id === nextId ? "down" : undefined
  const guides = treeGuides(visible)
  const listRef = useRef<HTMLDivElement>(null)

  const row = (t: AgentDetail, archivedRow?: boolean) => (
    <AgentRow
      key={t.id}
      t={t}
      selected={t.id === selectedId}
      onSelect={(id) => {
        if (!t.pending) onSelect(id)
      }}
      onArchive={onArchive}
      onPause={archivedRow ? undefined : onPause}
      onRename={archivedRow ? undefined : onRename}
      onDelete={archivedRow ? onDelete : undefined}
      archived={archivedRow}
      navHint={navHintOf(t.id)}
      navHeld={navHeld}
      guide={guides.get(t.id)}
    />
  )

  // Archived/deleted rows collapse in place; the rows below slide up.
  const rows = useDepartures(visible, showArchived ? "archived" : "live", (t) =>
    row(t, showArchived),
  )

  // `bg-surface-2` (`--card` is the selected row on top). Flush, no h-margin.
  return (
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
          <div ref={listRef} className="relative p-2">
            <SelectionGlide
              containerRef={listRef}
              selectedId={selectedId}
              layoutKey={showArchived ? "archived" : "live"}
            />
            {visible.length === 0 && <EmptyState showArchived={showArchived} />}

            {rows}
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

      <AgentSearchPalette
        open={searchOpen}
        onClose={() => onSearchOpenChange(false)}
        agents={agents}
        fleetId={fleetId}
        onSelect={(id) => {
          onSelect(id)
          onSearchOpenChange(false)
        }}
      />
    </aside>
  )
}

/** The "‹ Archived" back-link band, shown ONLY while viewing the archived set.
 *  Returns null on the normal view (its former cluster moved to the header rail). */
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

/** The empty placeholder shown when no agent is visible — copy adapts to which
 *  set (live/archived) is on screen. */
function EmptyState({ showArchived }: { showArchived: boolean }) {
  const message = showArchived ? "No archived agents." : "No agents yet."
  return <p className="px-2.5 py-6 text-center text-[11.5px] text-muted-foreground/55">{message}</p>
}

/** Agent title markup — ellipsis at rest, brightens + un-clips on row hover; motion via {@link useTitleMarquee}. */
function MarqueeTitle({ name, trackRef }: { name: string; trackRef: React.Ref<HTMLSpanElement> }) {
  return (
    <span className="min-w-0 flex-1 overflow-hidden">
      <span
        ref={trackRef}
        className="agent-title inline-block max-w-full truncate align-top text-[13px] font-medium text-foreground/90 group-hover:overflow-visible group-hover:text-clip group-hover:text-foreground"
      >
        {name}
      </span>
    </span>
  )
}

/** The row title's inline name editor; `onDone` fires on commit and cancel. */
function RowRenameInput({
  t,
  onRename,
  onDone,
}: {
  t: AgentDetail
  onRename: (id: string, name: string) => void
  onDone: () => void
}) {
  return (
    <RenameInput
      name={t.name}
      className="text-[13px] font-medium"
      onCommit={(name) => {
        onDone()
        onRename(t.id, name)
      }}
      onCancel={onDone}
    />
  )
}

function AgentRow({
  t,
  selected,
  onSelect,
  onArchive,
  onPause,
  onRename,
  onDelete,
  archived,
  navHint,
  navHeld,
  guide,
}: {
  t: AgentDetail
  selected: boolean
  onSelect: (id: string) => void
  onArchive: (id: string) => void
  onPause?: ((id: string) => void) | undefined
  onRename?: ((id: string, name: string) => void) | undefined
  onDelete?: ((id: string) => void) | undefined
  archived?: boolean | undefined
  /** Which loop-nav arrow badge this row shows while ⌘/Ctrl is held (T634). */
  navHint?: "up" | "down" | undefined
  /** Whether ⌘/Ctrl is currently held — gates the badge's visibility. */
  navHeld?: boolean | undefined
  /** hierarchy-line layout for this row ({@link treeGuides}) */
  guide?: TreeGuide | undefined
}) {
  const isPaused = !archived && t.paused
  const marquee = useTitleMarquee()
  const [renaming, setRenaming] = useState(false)

  return (
    <div
      onMouseEnter={marquee.onMouseEnter}
      onMouseLeave={marquee.onMouseLeave}
      className={cn(
        "group relative flex w-full items-stretch rounded-lg px-2.5 text-left transition-[background-color,scale] duration-300 ease-out select-none hover:duration-150 active:scale-[0.99]",
        !selected && "hover:bg-card/55",
      )}
      data-row-id={t.id}
    >
      {navHint && (
        <HintBadge label={navHint === "up" ? "↑" : "↓"} shown={Boolean(navHeld)} side="left" />
      )}
      <TreeGuides depth={t.depth ?? 0} guide={guide} />
      <div
        {...clickable(() => onSelect(t.id))}
        className="flex min-w-0 flex-1 items-center gap-2 py-1.5 text-left"
      >
        <StatusDot t={t} archived={Boolean(archived)} />
        {isPaused && <StatusBadge tone="warn" label="paused" />}
        {renaming && onRename ? (
          <RowRenameInput t={t} onRename={onRename} onDone={() => setRenaming(false)} />
        ) : (
          <MarqueeTitle name={t.name} trackRef={marquee.trackRef} />
        )}
        {t.pending && <PendingSpinner />}
        <span className="relative ml-auto flex shrink-0 items-center">
          <UnreadBadge count={archived ? 0 : (t.unread ?? 0)} />
          {!renaming && (
            <RowActions
              id={t.id}
              archived={Boolean(archived)}
              isPaused={Boolean(isPaused)}
              onArchive={onArchive}
              onDelete={onDelete}
              onPause={onPause}
              onRename={() => setRenaming(true)}
            />
          )}
        </span>
      </div>
    </div>
  )
}

/** Leading status square: brown when the backend's `lit` is set (TUI spinner
 *  or green circle), grey otherwise and on archived rows. */
function StatusDot({ t, archived }: { t: AgentDetail; archived: boolean }) {
  return <span className="size-2 shrink-0" style={{ background: litColor(t, archived) }} />
}

/** Tree indent for a row: one 12px column per depth level. Ancestor columns
 *  carry a line only while their branch continues below; the own column draws
 *  the upper half + elbow, plus the lower half when a sibling follows. A parent
 *  gets a stub from its dot down to its children's column. */
function TreeGuides({ depth, guide }: { depth: number; guide: TreeGuide | undefined }) {
  const line = "absolute left-1 w-px bg-muted-foreground/25"
  return (
    <>
      {depth > 0 && (
        <span aria-hidden className="flex shrink-0">
          {Array.from({ length: depth }, (_, i) => {
            const own = i === depth - 1
            return (
              <span key={i} className="relative w-3">
                {own ? (
                  <>
                    <span className={cn(line, "top-0", guide?.more ? "bottom-0" : "h-1/2")} />
                    <span className="absolute top-1/2 left-1 h-px w-2 bg-muted-foreground/25" />
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
          style={{ left: 14 + depth * 12 }}
        />
      )}
    </>
  )
}

/** Unread-count pill at the end of the row; hidden at zero and on row hover
 *  (the hover actions take its place). */
function UnreadBadge({ count }: { count: number }) {
  if (count <= 0) return null
  return (
    <span
      className="px-1.5 text-[10px] font-semibold text-(--primary-foreground) tabular-nums transition-opacity group-hover:opacity-0"
      style={{ background: "var(--signal)" }}
    >
      {count}
    </span>
  )
}

/** Hover-revealed row actions: archive/restore, delete (archived), rename + pause/resume (live). Each stops propagation so it doesn't select the row. */
function RowActions({
  id,
  archived,
  isPaused,
  onArchive,
  onDelete,
  onPause,
  onRename,
}: {
  id: string
  archived: boolean
  isPaused: boolean
  onArchive: (id: string) => void
  onDelete?: ((id: string) => void) | undefined
  onPause?: ((id: string) => void) | undefined
  /** opens the inline name editor */
  onRename?: (() => void) | undefined
}) {
  return (
    <span className="absolute inset-0 flex items-center justify-end gap-1 opacity-0 transition-opacity group-hover:opacity-100">
      {!archived && onRename && (
        <RowAction copy={ROW_ACTION_COPY.rename} icon={Pencil} onClick={onRename} />
      )}
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
    // `top`: `right` would open over the conversation, `left` back over the rail.
    <Tip title={copy.title} body={copy.body} side="top" triggerClassName="inline-flex">
      <button
        onClick={(e) => {
          // Without this, using an action would also select the row behind it.
          e.stopPropagation()
          onClick()
        }}
        aria-label={copy.title}
        className={cn(
          "flex size-5 cursor-pointer items-center justify-center rounded-md text-muted-foreground/70 transition-colors",
          danger ? "hover:text-(--danger)" : "hover:text-foreground",
        )}
      >
        <Icon className="size-3" />
      </button>
    </Tip>
  )
}

/** A first-line status pill (paused), tone-keyed to the palette (no `rounded-*` — the codebase is square). */
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
