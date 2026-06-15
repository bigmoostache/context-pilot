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
 * The sidebar collapses by dragging its right **edge rail** — the exact shadcn
 * Sidebar interaction used by the fleet dashboard (no in-rail button).
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
  const [collapsed, setCollapsed] = useState(false)
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
    <div className="relative flex min-h-0 flex-1">
      <ThreadList
        threads={threads}
        selectedId={thread?.id ?? ""}
        onSelect={setSelectedId}
        collapsed={collapsed}
        query={query}
        onQueryChange={setQuery}
        showArchived={showArchived}
        onToggleArchived={setShowArchived}
        onArchive={handleArchive}
        onNewThread={() => setNewOpen(true)}
      />

      {/* Collapse rail — click the sidebar's right edge to collapse/expand it
          (the same shadcn Sidebar interaction as the fleet dashboard). A
          generous hit zone hugs the border; on hover a soft band lights up and
          a pill-grip handle appears so the affordance reads clearly. Tracks the
          sidebar width and stays reachable at x≈0 when collapsed. */}
      <button
        onClick={() => setCollapsed((v) => !v)}
        title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        className="group absolute inset-y-0 z-20 w-5 -translate-x-1/2 cursor-pointer transition-[left] duration-200 ease-in-out"
        style={{ left: collapsed ? 6 : "var(--sidebar-w)" }}
      >
        {/* hover band — a subtle highlight across the seam */}
        <span className="absolute inset-y-0 left-1/2 w-[3px] -translate-x-1/2 rounded-full bg-border transition-all duration-150 group-hover:w-[5px] group-hover:bg-[var(--interactive)]/45 group-active:bg-[var(--interactive)]/70" />
        {/* pill-grip handle — the obvious drag/click affordance */}
        <span className="absolute left-1/2 top-1/2 flex h-11 w-[18px] -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border border-border bg-card opacity-0 shadow-sm transition-all duration-150 group-hover:opacity-100 group-active:scale-95">
          <span className="flex flex-col items-center gap-[3px]">
            <span className="size-[3px] rounded-full bg-muted-foreground/60" />
            <span className="size-[3px] rounded-full bg-muted-foreground/60" />
            <span className="size-[3px] rounded-full bg-muted-foreground/60" />
          </span>
        </span>
      </button>

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
