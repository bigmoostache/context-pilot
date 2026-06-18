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
  // Transient error notice for failed commands (matches the Finder/Fleet
  // flash pattern). Without this a 503/502 from sendCommand only reached
  // console.error — the composer cleared and the user saw nothing.
  const [notice, setNotice] = useState<string | null>(null)
  const flash = useCallback((msg: string) => {
    setNotice(msg)
    window.setTimeout(() => setNotice(null), 4500)
  }, [])

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
    sendCommand(activeAgentId, { kind, thread_id: id }).catch((e) => flash(humaniseCommandError(e)))
  }, [threads, activeAgentId, flash])

  const handleCreate = useCallback((title: string) => {
    sendCommand(activeAgentId, { kind: "create_thread", name: title.trim() || "Untitled thread" })
      .catch((e) => flash(humaniseCommandError(e)))
    setNewOpen(false)
    setQuery("")
    setShowArchived(false)
  }, [activeAgentId, flash])

  const handleSend = useCallback((text: string) => {
    if (!effectiveSelectedId || !text.trim()) return
    sendCommand(activeAgentId, {
      kind: "send_message",
      thread_id: effectiveSelectedId,
      content: text.trim(),
    }).catch((e) => flash(humaniseCommandError(e)))
  }, [activeAgentId, effectiveSelectedId, flash])

  if (!agent || threads.length === 0) {
    return <EmptyRealm agentName={agent?.name} />
  }

  return (
    <>
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

      {notice && (
        <div className="pointer-events-none fixed bottom-6 left-1/2 z-50 -translate-x-1/2">
          <div className="flex items-center gap-2 rounded-xl border border-[var(--danger)]/50 bg-card px-3.5 py-2 text-[12.5px] text-foreground/90 card-shadow">
            <span className="size-1.5 rounded-full bg-[var(--danger)]" />
            {notice}
          </div>
        </div>
      )}
    </>
  )
}

/**
 * Turn a raw `sendCommand` rejection into a short, human message. The backend
 * throws `"<status> <path>: <body>"`; we recognise the two failures a user can
 * actually hit and fall back to a terse generic for anything else.
 *
 * - **503 `tripped`** → the agent's CostBreaker is open (it crossed its spend
 *   budget). Cost-free control-plane commands now bypass the breaker (T114), so
 *   reaching this means a `send_message` to an over-budget agent — the breaker
 *   doing its job. The user's lever is raising/resetting the budget.
 * - **502** → the agent process is unreachable over its command socket.
 */
function humaniseCommandError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  if (msg.includes("tripped")) {
    return "This agent has reached its spend limit — raise its budget to send more messages."
  }
  if (msg.includes("502")) {
    return "Agent unreachable — it may be offline. Try again in a moment."
  }
  return `Couldn't complete that action${msg ? ` (${msg.slice(0, 60)})` : ""}.`
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
