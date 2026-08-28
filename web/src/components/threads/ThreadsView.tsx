import { FolderGit2, AlertTriangle, Plus } from "lucide-react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { NewThreadDialog } from "./dialogs/NewThreadDialog"
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
 * The thread list rail collapses from the header rail's Threads tab, and its
 * two actions — New thread, Search — live up there beside it, so neither
 * depends on the rail being visible.
 */
export function ThreadsView({
  activeAgentId,
  selectedThreadId,
  onThreadChange,
  onShowInFinder,
  railOpen,
  newOpen,
  onNewOpenChange,
  searchOpen,
  onSearchOpenChange,
  disconnected,
  onReconnect,
}: {
  activeAgentId: string
  /** Selected thread — CONTROLLED by the shell (Root.tsx) so browser Back/Next
   *  can step through the threads visited (T636). The shell owns the value and
   *  its per-agent persistence; this view feeds it into useThreadSelection. */
  selectedThreadId: string
  onThreadChange: (id: string) => void
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: (path: string) => void
  /** Whether the thread-list rail is shown. Owned by the shell (Root.tsx), not
   *  here: the only control that toggles it is the header rail's Threads tab,
   *  which is a SIBLING of this view rather than a descendant. */
  railOpen: boolean
  /** New Thread dialog open flag — shell-owned for the same reason as
   *  `railOpen`: its button sits in the header rail. The DIALOG still renders
   *  here, where the agent it creates against is in scope. */
  newOpen: boolean
  onNewOpenChange: (v: boolean) => void
  /** Search palette open flag — shell-owned, palette rendered down in the list. */
  searchOpen: boolean
  onSearchOpenChange: (v: boolean) => void
  disconnected?: boolean
  onReconnect?: () => void
}) {
  const { data: agents = [] } = useFleet()
  const { data: threads = [] } = useThreads(activeAgentId)
  const agent = agents.find((a) => a.id === activeAgentId)

  // The dialog flag is INJECTED into the selection hook rather than used
  // alongside it, so `handleCreate`'s own `setNewOpen(false)` closes the very
  // flag the rail's button opened.
  const sel = useThreadSelection(
    activeAgentId,
    threads,
    {
      open: newOpen,
      setOpen: onNewOpenChange,
    },
    { selectedId: selectedThreadId, setSelectedId: onThreadChange },
  )
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
      className="relative flex min-h-0 flex-1 overflow-hidden"
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
      {/* The rail stays mounted and slides in/out via an animated negative
          left-margin (the root's overflow-hidden clips it off-screen when
          closed); the sibling conversation grows to fill the reclaimed space.
          A margin transition — not a transform — is what actually reclaims the
          layout width, so the pane truly widens as the rail leaves.

          The offset is exactly the rail's width, with nothing added: ThreadList's
          aside carries no horizontal margin (`my-2` only). It used to be width
          PLUS those margins — a sum kept by hand here while the margins lived in
          another file, and it drifted twice. Should the aside ever regain a
          horizontal margin, this has to grow by the same amount or collapsing
          will leave a dead gutter where the rail used to be. */}
      <div
        // `flex` so the wrapped <aside> (which sizes itself from flex-stretch,
        // not an explicit height) still fills the row's full height — a plain
        // block wrapper collapses it to content height (T670 regression).
        className="flex shrink-0 transition-[margin-left] duration-300 ease-[cubic-bezier(.16,1,.3,1)] motion-reduce:transition-none"
        style={{ marginLeft: railOpen ? 0 : "calc(-1 * var(--sidebar-w))" }}
      >
        <ThreadList
          threads={threads}
          agentId={activeAgentId}
          selectedId={sel.effectiveSelectedId}
          onSelect={sel.setSelectedId}
          showArchived={sel.showArchived}
          onToggleArchived={sel.setShowArchived}
          onArchive={actions.handleArchive}
          onDelete={actions.handleDelete}
          onPause={actions.handlePause}
          searchOpen={searchOpen}
          onSearchOpenChange={onSearchOpenChange}
        />
      </div>

      {/* The floating collapsed-rail cluster that used to sit here is gone: it
          existed only to keep New thread + Search reachable while the rail was
          hidden, and both now live permanently in the header rail. */}

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
          leftRailHidden={!railOpen}
          onUnarchive={() => {
            actions.handleArchive(thread.id)
            sel.setShowArchived(false)
          }}
        />
      ) : (
        <EmptyRealm agentName={agent.name} onNewThread={() => sel.setNewOpen(true)} />
      )}

      <NewThreadDialog
        open={sel.newOpen}
        onClose={() => sel.setNewOpen(false)}
        onCreate={actions.handleCreate}
        agentId={activeAgentId}
      />

      {actions.notice && (
        <div
          role="alert"
          className={
            "card-shadow fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-xl border bg-card px-4 py-2.5 text-[12.5px] text-foreground/90 " +
            (actions.notice.tone === "error" ? "border-(--danger)/40" : "border-border")
          }
        >
          {actions.notice.tone === "error" && (
            <AlertTriangle className="size-4 shrink-0 text-(--danger)" />
          )}
          <span>{actions.notice.message}</span>
          {actions.notice.undo && (
            <button
              onClick={actions.notice.undo}
              className="ml-1 shrink-0 rounded-md px-2 py-0.5 text-[12px] font-medium text-(--signal) transition-colors hover:bg-(--signal)/10"
            >
              Undo
            </button>
          )}
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
