import { useState, useCallback, useRef, useEffect } from "react"
import { FolderGit2, AlertTriangle } from "lucide-react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { NewThreadDialog } from "./NewThreadDialog"
import { useThreads, useFleet, sendCommand } from "@/lib/live"

/**
 * Turn a rejected `sendCommand` into a human sentence for the notice toast.
 *
 * `api.request` throws `Error("<status> <path>: <body>")` on any non-2xx, so a
 * tripped **CostBreaker** surfaces as a `503` whose body carries
 * `{"status":"tripped"}` (design doc R2-8 / V9). That case gets a specific,
 * actionable message — the silent-failure hole behind T121, where an
 * over-budget send was swallowed by `.catch(console.error)` and the user saw
 * nothing happen. Every other failure degrades to a generic, still-visible
 * line so a command is *never* silently dropped again.
 */
function describeCommandError(verb: string, err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err)
  if (msg.includes("503") || msg.toLowerCase().includes("tripped")) {
    return "Send blocked — this agent is over its spend budget (cost breaker tripped). Raise the budget or stop the run, then try again."
  }
  return `Could not ${verb}: ${msg}`
}

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

  // Transient, breaker-aware failure notice. A command rejected by the backend
  // (most importantly a tripped CostBreaker → 503) must be *visible*, never the
  // old silent `.catch(console.error)` swallow (T121). One pending timer at a
  // time; cleared on unmount so a late tick can't setState a dead component.
  const [notice, setNotice] = useState<string | null>(null)
  const noticeTimer = useRef<number | null>(null)
  const flash = useCallback((msg: string) => {
    if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current)
    setNotice(msg)
    noticeTimer.current = window.setTimeout(() => setNotice(null), 6000)
  }, [])
  useEffect(
    () => () => {
      if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current)
    },
    [],
  )

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
    const verb = t.archived ? "restore the thread" : "archive the thread"
    sendCommand(activeAgentId, { kind, thread_id: id }).catch((e) =>
      flash(describeCommandError(verb, e)),
    )
  }, [threads, activeAgentId, flash])

  const handleCreate = useCallback((title: string) => {
    sendCommand(activeAgentId, { kind: "create_thread", name: title.trim() || "Untitled thread" })
      .catch((e) => flash(describeCommandError("create the thread", e)))
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
    }).catch((e) => flash(describeCommandError("send your message", e)))
  }, [activeAgentId, effectiveSelectedId, flash])

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

      {notice && (
        <div
          role="alert"
          className="fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-xl border border-[var(--danger)]/40 bg-card px-4 py-2.5 text-[12.5px] text-foreground/90 card-shadow"
        >
          <AlertTriangle className="size-4 shrink-0 text-[var(--danger)]" />
          <span>{notice}</span>
        </div>
      )}
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
