import { useState } from "react"
import { FolderGit2 } from "lucide-react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { NewThreadDialog } from "./NewThreadDialog"
import { threadDetails, agents } from "@/lib/mock"
import type { ThreadDetail } from "@/lib/types"

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
  const agent = agents.find((a) => a.id === activeAgentId)

  const [threads, setThreads] = useState<ThreadDetail[]>(() =>
    threadDetails.filter((t) => t.agentId === activeAgentId),
  )
  const [selectedId, setSelectedId] = useState(
    () => threads.find((t) => !t.archived)?.id ?? threads[0]?.id ?? "",
  )
  const [query, setQuery] = useState("")
  const [showArchived, setShowArchived] = useState(false)
  const [newOpen, setNewOpen] = useState(false)

  const thread = threads.find((t) => t.id === selectedId) ?? threads[0]

  // archive ↔ restore; keep the selection valid if we just archived it
  const handleArchive = (id: string) => {
    setThreads((prev) => {
      const next = prev.map((t) => (t.id === id ? { ...t, archived: !t.archived } : t))
      if (id === selectedId) {
        const fallback = next.find((t) => !t.archived && t.id !== id) ?? next.find((t) => t.id !== id)
        if (fallback) setSelectedId(fallback.id)
      }
      return next
    })
  }

  const handleCreate = (title: string) => {
    const id = `local-${Date.now()}`
    const created: ThreadDetail = {
      id,
      name: title.trim() || "Untitled thread",
      status: "MY_TURN",
      agentId: activeAgentId,
      agent: agent?.name ?? "agent",
      createdAt: "just now",
      lastActivity: "just now",
      unread: 0,
      log: [],
    }
    setThreads((prev) => [created, ...prev])
    setSelectedId(id)
    setShowArchived(false)
    setQuery("")
    setNewOpen(false)
  }

  if (!agent || threads.length === 0) {
    return <EmptyRealm agentName={agent?.name} />
  }

  return (
    <div className="flex min-h-0 flex-1">
      <ThreadList
        threads={threads}
        selectedId={thread?.id ?? ""}
        onSelect={setSelectedId}
        query={query}
        onQueryChange={setQuery}
        showArchived={showArchived}
        onToggleArchived={setShowArchived}
        onArchive={handleArchive}
        onNewThread={() => setNewOpen(true)}
      />

      {thread && <ThreadConversation thread={thread} />}

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
