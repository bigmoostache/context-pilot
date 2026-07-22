import { useCallback, useState } from "react"
import { FolderGit2, AlertTriangle, Plus, PanelLeft } from "lucide-react"
import { ThreadList } from "@/mobile-components/threads/ThreadList"
import { ThreadConversation } from "@/mobile-components/threads/ThreadConversation"
import { NewThreadDialog } from "@/mobile-components/threads/NewThreadDialog"
import { useFleet, useThreads } from "@/lib/live"
import { useThreadSelection, useThreadActions } from "@/lib/live/threadView"

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
  disconnected,
  onReconnect,
}: {
  activeAgentId: string
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: (path: string) => void
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
          onNewThread={() => sel.setNewOpen(true)}
        />
      )}

      {/* Fixed toggle — top-left of the view (below the shell TopBar). Sits under
          the drawer scrim (lower z), so tapping it while the drawer is open lands
          on the scrim and closes instead. */}
      <button
        onClick={() => setDrawerOpen(true)}
        aria-label="Show threads"
        className="card-shadow absolute top-2 left-2 z-30 flex size-9 items-center justify-center rounded-lg border border-border bg-card/95 text-foreground/80 backdrop-blur-sm transition-colors active:bg-muted"
      >
        <PanelLeft className="size-4.5" />
      </button>

      {/* Scrim — dims the conversation while the drawer is open; tapping it (or
          the toggle beneath it) closes the drawer. A <button> not a <div> so it
          carries keyboard semantics (jsx-a11y). */}
      <button
        aria-label="Close thread list"
        onClick={() => setDrawerOpen(false)}
        tabIndex={drawerOpen ? 0 : -1}
        className={
          "absolute inset-0 z-40 bg-black/40 transition-opacity duration-200 " +
          (drawerOpen ? "opacity-100" : "pointer-events-none opacity-0")
        }
      />

      {/* Drawer — the thread list, sliding in from the left. Always mounted (so
          it animates) but shoved off-screen and non-interactive when closed. */}
      <aside
        aria-hidden={!drawerOpen}
        className={
          "absolute inset-y-0 left-0 z-50 flex w-[85%] max-w-[340px] flex-col border-r border-border bg-surface transition-transform duration-200 ease-out " +
          (drawerOpen ? "translate-x-0" : "-translate-x-full")
        }
      >
        <ThreadList
          threads={threads}
          selectedId={sel.effectiveSelectedId}
          onSelect={openThread}
          query={sel.query}
          onQueryChange={sel.setQuery}
          showArchived={sel.showArchived}
          onToggleArchived={sel.setShowArchived}
          onArchive={actions.handleArchive}
          onDelete={actions.handleDelete}
          onPause={actions.handlePause}
          onNewThread={() => sel.setNewOpen(true)}
        />
      </aside>

      <NewThreadDialog
        open={sel.newOpen}
        onClose={() => sel.setNewOpen(false)}
        onCreate={(name) => {
          actions.handleCreate(name)
          setDrawerOpen(false)
        }}
      />

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
