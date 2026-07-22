import { useCallback, useEffect, useRef, useState } from "react"
import { animate, createSpring } from "animejs"
import { FolderGit2, AlertTriangle, Plus, PanelLeft } from "lucide-react"
import { ThreadList } from "@/mobile-components/threads/ThreadList"
import { ThreadConversation } from "@/mobile-components/threads/ThreadConversation"
import { CornerButton } from "@/mobile-components/shell/CornerButton"
import { useFleet, useThreads } from "@/lib/live"
import { useThreadSelection, useThreadActions } from "@/lib/live/threadView"
import { prefersReducedMotion } from "@/lib/utils"

/**
 * Mobile thread surface — the divergent twin of `components/threads/ThreadsView`.
 *
 * The desktop view is a two-pane master-detail (a permanent list rail beside the
 * conversation). At phone width a permanent rail steals the entire screen from
 * the conversation and makes reading it impossible (T609), so mobile forks the
 * SAME behaviour into a **conversation-primary + drawer** layout: the
 * conversation is the full-bleed base layer, and the thread list lives in a
 * slide-in left drawer toggled by a fixed top-left button (hamburger). Opening a
 * thread closes the drawer, handing the whole screen back to the conversation.
 *
 * Only the presentation differs; every piece of logic (selection persistence,
 * archive/pause/delete/create/send/attach, the failure notice) comes from the
 * shared `@/lib/live/threadView` hooks the desktop view also consumes, so the
 * two trees can never drift (design-mobile.md §3.2, architecture rule M141).
 */
export function ThreadsView({
  activeAgentId,
  onShowInFinder,
  onGoToAgents,
  disconnected,
  onReconnect,
}: {
  activeAgentId: string
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: (path: string) => void
  /** leave the conversation for the agents (fleet) page — wired to the thread
   *  drawer's top-left corner button (T631) */
  onGoToAgents?: (() => void) | undefined
  disconnected?: boolean
  onReconnect?: () => void
}) {
  const { data: agents = [] } = useFleet()
  const { data: threads = [] } = useThreads(activeAgentId)
  const hasAgent = agents.some((a) => a.id === activeAgentId)

  const sel = useThreadSelection(activeAgentId, threads)
  const actions = useThreadActions(activeAgentId, threads, sel)

  // The one piece of state desktop's always-open rail doesn't need: whether the
  // thread-list drawer is showing. Closed by default when a realm opens (the
  // conversation owns the screen); the user taps the top-left toggle to browse
  // threads, and picking one closes it again.
  const [drawerOpen, setDrawerOpen] = useState(false)

  const openThread = useCallback(
    (id: string) => {
      sel.setSelectedId(id)
      setDrawerOpen(false)
    },
    [sel],
  )

  // #2 Drawer settle (anime.js): spring the drawer's slide instead of the flat
  // CSS transition, for the subtle iOS overshoot-and-settle. anime owns the
  // aside's transform (translateX 0% ↔ -100%); the first run is skipped (the
  // drawer mounts closed off-screen — a spring there would flash it in) and
  // reduced-motion snaps to the end position. The `-translate-x-full` class is
  // dropped from the aside so React and anime never both write transform.
  const drawerRef = useRef<HTMLElement>(null)
  const firstRunRef = useRef(true)
  useEffect(() => {
    const el = drawerRef.current
    if (!el) return
    const to = drawerOpen ? "0%" : "-100%"
    if (firstRunRef.current || prefersReducedMotion()) {
      firstRunRef.current = false
      el.style.transform = `translateX(${to})`
      return
    }
    animate(el, { translateX: to, ease: createSpring({ stiffness: 300, damping: 30 }) })
  }, [drawerOpen])

  // No agent at all → bare empty state (no realm to render a roster for).
  if (!hasAgent) {
    return <EmptyRealm agentName={undefined} />
  }

  const thread = threads.find((t) => t.id === sel.effectiveSelectedId)

  return (
    <div
      className="relative flex min-h-0 flex-1 flex-col"
      style={
        disconnected
          ? { filter: "blur(3px) grayscale(0.5)", transition: "filter 300ms" }
          : { transition: "filter 300ms" }
      }
    >
      {disconnected && (
        <button
          onClick={onReconnect}
          className="absolute inset-0 z-40 cursor-pointer bg-background/30"
          aria-label="Reconnect to agent"
        />
      )}

      {/* Base layer: the conversation (or an empty-realm hint). Full-bleed — the
          list no longer shares the screen with it. */}
      {thread ? (
        <ThreadConversation
          thread={thread}
          agentId={activeAgentId}
          onSend={actions.handleSend}
          onAttach={actions.handleAttach}
          pendingFiles={sel.pendingFiles}
          onRemoveFile={(i) => sel.setPendingFiles((prev) => prev.filter((_, idx) => idx !== i))}
          onShowInFinder={onShowInFinder}
        />
      ) : (
        <EmptyRealm
          agentName={agents.find((a) => a.id === activeAgentId)?.name}
          onNewThread={() => setDrawerOpen(true)}
        />
      )}

      {/* Drawer toggle — the shared top-left corner button (see CornerButton:
          fixed, safe-area-offset so it clears the iOS status bar in standalone,
          T621). It carries `z-20` so it sits UNDER the drawer scrim (z-40): a
          tap while the drawer is open lands on the scrim and closes it. */}
      <CornerButton
        side="left"
        label="Show threads"
        onClick={() => setDrawerOpen(true)}
        className="z-20"
      >
        <PanelLeft className="size-4.5" />
      </CornerButton>

      {/* Scrim — dims the conversation while the drawer is open; tapping it (or
          the toggle beneath it) closes the drawer. A <button> not a <div> so it
          carries keyboard semantics (jsx-a11y). `fixed` (not `absolute`) so it
          covers the viewport, not the tall scrolling ThreadsView container. */}
      <button
        aria-label="Close thread list"
        onClick={() => setDrawerOpen(false)}
        tabIndex={drawerOpen ? 0 : -1}
        className={
          "fixed inset-0 z-40 bg-black/40 transition-opacity duration-200 " +
          (drawerOpen ? "opacity-100" : "pointer-events-none opacity-0")
        }
      />

      {/* Drawer — the thread list, sliding in from the left. Always mounted (so
          it animates) but shoved off-screen and non-interactive when closed.
          `fixed inset-y-0` pins it to the VIEWPORT height, not the tall
          scrolling ThreadsView container beneath it — so ThreadList's own
          ScrollArea scrolls independently of the conversation (T616). Full
          viewport width (`inset-0`, not a partial `w-[85%]`) — on a phone a
          sliver of the dimmed conversation peeking at the edge just cramps the
          list for no benefit (T619). */}
      <aside
        ref={drawerRef}
        aria-hidden={!drawerOpen}
        style={{ transform: "translateX(-100%)" }}
        className={
          "fixed inset-0 z-50 flex flex-col bg-surface " +
          (drawerOpen ? "" : "pointer-events-none")
        }
      >
        <ThreadList
          threads={threads}
          selectedId={sel.effectiveSelectedId}
          onSelect={openThread}
          onGoToAgents={onGoToAgents}
          query={sel.query}
          onQueryChange={sel.setQuery}
          showArchived={sel.showArchived}
          onToggleArchived={sel.setShowArchived}
          onArchive={actions.handleArchive}
          onDelete={actions.handleDelete}
          onPause={actions.handlePause}
          onNewThread={(name) => {
            actions.handleCreate(name)
            setDrawerOpen(false)
          }}
        />
      </aside>

      {actions.notice && (
        <div
          role="alert"
          className="card-shadow fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-xl border border-(--danger)/40 bg-card px-4 py-2.5 text-[12.5px] text-foreground/90"
        >
          <AlertTriangle className="size-4 shrink-0 text-(--danger)" />
          <span>{actions.notice}</span>
        </div>
      )}
    </div>
  )
}

/** Shown when there is no agent (no realm) or no thread picked yet — the mobile
 *  twin of the desktop EmptyRealm. When `onNewThread` is supplied it offers a
 *  primary action so an empty realm can bootstrap its first thread. */
function EmptyRealm({
  agentName,
  onNewThread,
}: {
  agentName?: string | undefined
  onNewThread?: (() => void) | undefined
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 bg-background px-6 text-center">
      <span className="flex size-12 items-center justify-center rounded-2xl bg-muted text-muted-foreground/60">
        <FolderGit2 className="size-6" />
      </span>
      <p className="max-w-[320px] text-[13px] text-muted-foreground">
        {agentName ? (
          <>
            <span className="font-medium text-foreground/80">{agentName}</span> has no threads yet —
            tap the menu (top-left) to browse, or start one to put it to work in its folder.
          </>
        ) : (
          "Select an agent to see its threads."
        )}
      </p>
      {onNewThread && (
        <button
          onClick={onNewThread}
          className="flex items-center gap-2 rounded-lg bg-(--signal) px-3.5 py-2 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105"
        >
          <Plus className="size-4" />
          New Thread
        </button>
      )}
    </div>
  )
}
