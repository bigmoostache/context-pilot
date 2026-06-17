import { useState, useCallback } from "react"
import { FolderGit2 } from "lucide-react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { NewThreadDialog } from "./NewThreadDialog"
import { useThreads, useFleet, sendCommand } from "@/lib/live"

/**
 * Thread-centered view — the conversation-first layout: thread list (left) |
 * conversation (center). Scoped to the **active agent's realm**: an agent lives
 * in its folder and owns the threads inside it, so we only ever show that
 * agent's threads — never a cross-agent global list.
 *
 * This component is keyed by `activeAgentId` in {@link App}, so its local
 * thread state is reseeded (fresh mount) whenever the realm changes. Local
 * state lets the maquette's interactions genuinely *work*: the search box
 * filters, **New Thread** prepends a thread, and **archive / restore** moves
 * threads in and out of the archived view.
 *
 * The thread list is **always open** — there is no collapse/expand affordance
 * (removed per T23); the rail is a permanent fixture of the threads view.
 */
export function ThreadsView({
  activeAgentId,
}: {
  activeAgentId: string
}) {
  const { data: agents = [] } = useFleet()
  const { data: threads = [] } = useThreads(activeAgentId)
  const agent = agents.find((a) => a.id === activeAgentId)

  const [selectedId, setSelectedId] = useState("")
  const [query, setQuery] = useState("")
  const [showArchived, setShowArchived] = useState(false)
  const [newOpen, setNewOpen] = useState(false)

  // Auto-select first non-archived thread if current selection is invalid
  const validSelection = threads.some((t) => t.id === selectedId)
  const effectiveSelectedId = validSelection
    ? selectedId
    : threads.find((t) => !t.archived)?.id ?? threads[0]?.id ?? ""

  const thread = threads.find((t) => t.id === effectiveSelectedId)

  const handleArchive = useCallback((id: string) => {
    const t = threads.find((th) => th.id === id)
    if (!t) return
    const kind = t.archived ? "restore_thread" : "archive_thread"
    sendCommand(activeAgentId, { kind, thread_id: id }).catch(console.error)
  }, [threads, activeAgentId])

  const handleCreate = useCallback((title: string) => {
    sendCommand(activeAgentId, { kind: "create_thread", name: title.trim() || "Untitled thread" })
      .catch(console.error)
    setNewOpen(false)
    setQuery("")
    setShowArchived(false)
  }, [activeAgentId])

  const handleSend = useCallback((text: string) => {
    if (!effectiveSelectedId || !text.trim()) return
    sendCommand(activeAgentId, {
      kind: "send_message",
      thread_id: effectiveSelectedId,
      content: text.trim(),
    }).catch(console.error)
  }, [activeAgentId, effectiveSelectedId])

  if (!agent || threads.length === 0) {
    return <EmptyRealm agentName={agent?.name} />
  }

  return (
    <div className="flex min-h-0 flex-1">
      <ThreadList
        threads={threads}
        selectedId={effectiveSelectedId}
        onSelect={setSelectedId}
        query={query}
        onQueryChange={setQuery}
        showArchived={showArchived}
        onToggleArchived={setShowArchived}
        onArchive={handleArchive}
        onNewThread={() => setNewOpen(true)}
      />

      {thread && <ThreadConversation thread={thread} onSend={handleSend} />}

      <NewThreadDialog open={newOpen} onClose={() => setNewOpen(false)} onCreate={handleCreate} />
    </div>
  )
}

/** Shown when the active agent's realm holds no threads yet. */
function EmptyRealm({ agentName }: { agentName?: string }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 bg-background text-center">
      <span className="flex size-12 items-center justify-center rounded-2xl bg-muted text-muted-foreground/60">
        <FolderGit2 className="size-6" />
      </span>
      <p className="max-w-[320px] text-[13px] text-muted-foreground">
        {agentName ? (
          <>
            <span className="font-medium text-foreground/80">{agentName}</span> has no
            threads yet — start one to put it to work in its folder.
          </>
        ) : (
          "Select an agent to see its threads."
        )}
      </p>
    </div>
  )
}
