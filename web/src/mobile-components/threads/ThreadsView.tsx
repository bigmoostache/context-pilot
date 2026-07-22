import { useState, useCallback } from "react"
import { FolderGit2, AlertTriangle, Plus, ChevronLeft } from "lucide-react"
import { ThreadList } from "@/mobile-components/threads/ThreadList"
import { ThreadConversation } from "@/mobile-components/threads/ThreadConversation"
import { NewThreadDialog } from "@/mobile-components/threads/NewThreadDialog"
import { useFleet, useThreads } from "@/lib/live"
import { useThreadSelection, useThreadActions } from "@/lib/live/threadView"

/**
 * Mobile thread surface — the divergent twin of `components/threads/ThreadsView`.
 *
 * The desktop view is a two-pane master-detail (list rail + conversation side by
 * side); at phone width there is no room for both, so mobile forks the SAME
 * behaviour into **stack navigation**: a full-width list screen, and — once a
 * thread is opened — a full-width conversation screen with a back affordance.
 * Only the presentation differs; every piece of logic (selection persistence,
 * archive/pause/delete/create/send/attach, the failure notice) comes from the
 * shared `@/lib/live/threadView` hooks the desktop view also consumes, so the
 * two trees can never drift (design-mobile.md §3.2, architecture rule M141).
 *
 * Child components resolve through the `@/mobile-components/…` token (mirror
 * leak-guard): they are stub twins today (byte-identical to desktop), and become
 * real mobile layouts as this folder's recode proceeds — this view is already
 * wired to whichever version the mirror resolves.
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

  // Stack-nav position — the one piece of state desktop's side-by-side layout
  // doesn't need. "list" shows the roster; "conversation" shows the opened
  // thread. Opening a thread pushes; the back control pops.
  const [screen, setScreen] = useState<"list" | "conversation">("list")

  const openThread = useCallback(
    (id: string) => {
      sel.setSelectedId(id)
      setScreen("conversation")
    },
    [sel],
  )
  const back = useCallback(() => setScreen("list"), [])

  // No agent at all → bare empty state (no realm to render a roster for).
  if (!hasAgent) {
    return <EmptyRealm agentName={undefined} />
  }

  const thread = threads.find((t) => t.id === sel.effectiveSelectedId)
  // A conversation screen with no resolvable thread (stale/empty selection)
  // falls back to the list so the user is never stranded on a blank pane.
  const showConversation = screen === "conversation"

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

      {showConversation && thread ? (
        <>
          <ConversationHeader title={thread.name} onBack={back} />
          <ThreadConversation
            thread={thread}
            agentId={activeAgentId}
            onSend={actions.handleSend}
            onAttach={actions.handleAttach}
            pendingFiles={sel.pendingFiles}
            onRemoveFile={(i) => sel.setPendingFiles((prev) => prev.filter((_, idx) => idx !== i))}
            onShowInFinder={onShowInFinder}
          />
        </>
      ) : (
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
      )}

      <NewThreadDialog
        open={sel.newOpen}
        onClose={() => sel.setNewOpen(false)}
        onCreate={actions.handleCreate}
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

/** Slim conversation-screen header for the stack: a back control (pops to the
 *  list) plus the thread title. Replaces the desktop's always-visible list rail,
 *  which is the affordance mobile trades for full-width message space. */
function ConversationHeader({ title, onBack }: { title: string; onBack: () => void }) {
  return (
    <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-2">
      <button
        onClick={onBack}
        aria-label="Back to threads"
        className="flex size-8 items-center justify-center rounded-md text-foreground/80 transition-colors hover:bg-muted/60"
      >
        <ChevronLeft className="size-5" />
      </button>
      <span className="truncate text-[14px] font-semibold tracking-tight">{title}</span>
    </header>
  )
}

/** Shown when there is no agent (no realm) — the mobile twin of the desktop
 *  EmptyRealm. When `onNewThread` is supplied it offers a primary action so an
 *  empty realm can bootstrap its first thread. */
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
            start one to put it to work in its folder.
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
