import { FolderGit2, AlertTriangle, Plus } from "lucide-react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { NewThreadDialog } from "./NewThreadDialog"
import { useFleet, useThreads } from "@/lib/live"
import { useThreadSelection, useThreadActions } from "@/lib/live/threadView"

/**
 * Thread-centered view — the conversation-first layout: thread list (left) |
 * conversation (center). Scoped to the **active agent's realm**: an agent lives
 * in its folder and owns the threads inside it, so we only ever show that
 * agent's threads — never a cross-agent global list.
 *
 * This component is keyed by `activeAgentId` in {@link App}, so its local
 * thread state is reseeded (fresh mount) whenever the realm changes. Its logic
 * lives in two same-file hooks — {@link useThreadSelection} (selection + view
 * state) and {@link useThreadActions} (mutation handlers + notice) — so the
 * render body itself stays within the P8 budgets.
 *
 * The thread list is **always open** — there is no collapse/expand affordance
 * (removed per T23); the rail is a permanent fixture of the threads view.
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
  const agent = agents.find((a) => a.id === activeAgentId)

  const sel = useThreadSelection(activeAgentId, threads)
  const actions = useThreadActions(activeAgentId, threads, sel)

  // Only bail to a bare empty state when there is genuinely no agent. A fresh
  // agent with zero threads MUST still render the sidebar — that is where the
  // "New Thread" button lives — otherwise the realm is a dead end.
  if (!agent) {
    return <EmptyRealm agentName={undefined} />
  }

  const thread = threads.find((t) => t.id === sel.effectiveSelectedId)

  return (
    <div
      className="relative flex min-h-0 flex-1"
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
      <ThreadList
        threads={threads}
        selectedId={sel.effectiveSelectedId}
        onSelect={sel.setSelectedId}
        query={sel.query}
        onQueryChange={sel.setQuery}
        showArchived={sel.showArchived}
        onToggleArchived={sel.setShowArchived}
        onArchive={actions.handleArchive}
        onDelete={actions.handleDelete}
        onPause={actions.handlePause}
        onNewThread={() => sel.setNewOpen(true)}
      />

      {/* The conversation pane shows the selected thread, or — for a realm with
          no thread selected/created yet — a hint pointing at the sidebar's New
          Thread button so an empty realm is never a dead end. */}
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
        <EmptyRealm agentName={agent.name} onNewThread={() => sel.setNewOpen(true)} />
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

/** Shown in the conversation pane when no thread is selected — either the
 *  realm is empty, or nothing is picked yet. When `onNewThread` is supplied it
 *  offers a primary action so an empty realm can bootstrap its first thread
 *  without hunting for the sidebar button. */
function EmptyRealm({
  agentName,
  onNewThread,
}: {
  agentName?: string | undefined
  onNewThread?: (() => void) | undefined
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 bg-background text-center">
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
