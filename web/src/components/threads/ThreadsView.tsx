import { useState, useCallback, useRef, useEffect, useMemo } from "react"
import { FolderGit2, AlertTriangle, Plus } from "lucide-react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { NewThreadDialog } from "./NewThreadDialog"
import { useThreads, useFleet, sendCommand } from "@/lib/live"
import { uploadUnique } from "@/lib/api"
import { buildUploadMessage, type UploadedFile } from "./fileUpload/helpers"
import type { ThreadDetail } from "@/lib/types"

/**
 * Build a combined message body from user text and pending file attachments.
 * Text comes first (if any), then file blocks. Either can be absent — a
 * send with only pending files produces just the file blocks; one with only
 * text produces just text.
 */
function buildCombinedContent(text: string, files: UploadedFile[]): string {
  const parts: string[] = []
  if (text.trim()) parts.push(text.trim())
  if (files.length > 0) parts.push(buildUploadMessage(files))
  return parts.join("\n\n")
}

/**
 * Turn a rejected `sendCommand` into a human sentence for the notice toast.
 *
 * Every failure is surfaced visibly so a command is never silently dropped.
 */
function describeCommandError(verb: string, err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err)
  return `Could not ${verb}: ${msg}`
}

/** The thread-selection surface owned by {@link useThreadSelection}. */
interface Selection {
  selectedId: string
  setSelectedId: (id: string) => void
  query: string
  setQuery: (q: string) => void
  showArchived: boolean
  setShowArchived: (v: boolean) => void
  newOpen: boolean
  setNewOpen: (v: boolean) => void
  pendingFiles: UploadedFile[]
  setPendingFiles: React.Dispatch<React.SetStateAction<UploadedFile[]>>
  /** the resolved-to-a-real-thread id (selection may point at a stale/archived row) */
  effectiveSelectedId: string
  /** flag the next threads update to auto-select the newly-created thread */
  armAutoSelect: () => void
  /** cancel a pending auto-select (create failed before the id arrived) */
  disarmAutoSelect: () => void
}

/**
 * Own the thread selection + composer-adjacent view state for a realm.
 *
 * Selection is remembered PER AGENT in localStorage so a view switch (Finder ↔
 * threads) or a reload returns to the same thread (T303); the persisted value
 * is always the *effective* (resolved-to-existing) id. A just-created thread is
 * auto-selected once its server-assigned id arrives via the next SSE delta
 * (`armAutoSelect` sets the flag; the diff effect picks the newcomer). Switching
 * threads clears staged uploads via a render-phase reset (React's documented
 * adjust-state-on-prop-change pattern — not an effect, which would cost an extra
 * commit and trip set-state-in-effect).
 */
function useThreadSelection(activeAgentId: string, threads: ThreadDetail[]): Selection {
  const threadKey = `cp-thread-${activeAgentId}`
  const [selectedId, setSelectedId] = useState(() => localStorage.getItem(threadKey) ?? "")
  const [query, setQuery] = useState("")
  const [showArchived, setShowArchived] = useState(false)
  const [newOpen, setNewOpen] = useState(false)
  const [pendingFiles, setPendingFiles] = useState<UploadedFile[]>([])

  // Auto-select a just-created thread once its server-assigned id lands.
  const pendingSelectRef = useRef(false)
  const prevThreadIdsRef = useRef<Set<string>>(new Set())
  const currentIds = useMemo(() => new Set(threads.map((t) => t.id)), [threads])
  useEffect(() => {
    if (pendingSelectRef.current && threads.length > 0) {
      const newId = threads.find((t) => !prevThreadIdsRef.current.has(t.id))?.id
      if (newId) {
        setSelectedId(newId)
        pendingSelectRef.current = false
      }
    }
    prevThreadIdsRef.current = currentIds
  }, [threads, currentIds])

  // Resolve the selection to a thread that actually exists, falling back to the
  // first non-archived (then any) thread.
  const validSelection = threads.some((t) => t.id === selectedId)
  const effectiveSelectedId = validSelection
    ? selectedId
    : (threads.find((t) => !t.archived)?.id ?? threads[0]?.id ?? "")

  // Persist the RESOLVED selection so a reload returns to a still-existing thread.
  useEffect(() => {
    if (effectiveSelectedId) localStorage.setItem(threadKey, effectiveSelectedId)
  }, [effectiveSelectedId, threadKey])

  // Clear staged uploads when the thread changes (render-phase reset).
  const [pendingThread, setPendingThread] = useState(effectiveSelectedId)
  if (pendingThread !== effectiveSelectedId) {
    setPendingThread(effectiveSelectedId)
    setPendingFiles([])
  }

  return {
    selectedId,
    setSelectedId,
    query,
    setQuery,
    showArchived,
    setShowArchived,
    newOpen,
    setNewOpen,
    pendingFiles,
    setPendingFiles,
    effectiveSelectedId,
    armAutoSelect: () => {
      pendingSelectRef.current = true
    },
    disarmAutoSelect: () => {
      pendingSelectRef.current = false
    },
  }
}

/** The command handlers + failure notice returned by {@link useThreadActions}. */
interface Actions {
  notice: string | null
  handleArchive: (id: string) => void
  handlePause: (id: string) => void
  handleDelete: (id: string) => void
  handleCreate: (title: string) => void
  handleSend: (text: string) => void
  handleAttach: (files: File[]) => void | Promise<void>
}

/**
 * All thread-mutation handlers for a realm, plus a transient, breaker-aware
 * failure notice.
 *
 * A command rejected by the backend (most importantly a tripped CostBreaker →
 * 503) must be *visible*, never a silent `.catch(console.error)` swallow (T121):
 * every handler routes its rejection through {@link describeCommandError} into
 * `flash`, which shows one auto-dismissing toast at a time (cleared on unmount
 * so a late tick can't setState a dead component). Extracted from
 * {@link ThreadsView} so both units stay within the P8 line/statement budgets.
 */
function useThreadActions(activeAgentId: string, threads: ThreadDetail[], sel: Selection): Actions {
  const { selectedId, setSelectedId, effectiveSelectedId, pendingFiles, setPendingFiles } = sel

  const [notice, setNotice] = useState<string | null>(null)
  const noticeTimerRef = useRef<number | null>(null)
  const flash = useCallback((msg: string) => {
    if (noticeTimerRef.current !== null) window.clearTimeout(noticeTimerRef.current)
    setNotice(msg)
    noticeTimerRef.current = window.setTimeout(() => setNotice(null), 6000)
  }, [])
  useEffect(
    () => () => {
      if (noticeTimerRef.current !== null) window.clearTimeout(noticeTimerRef.current)
    },
    [],
  )

  const handleArchive = useCallback(
    (id: string) => {
      const t = threads.find((th) => th.id === id)
      if (!t) return
      const kind = t.archived ? "restore_thread" : "archive_thread"
      const verb = t.archived ? "restore the thread" : "archive the thread"
      // Deselect the archived thread so the view falls through to the next one.
      if (!t.archived && id === selectedId) setSelectedId("")
      sendCommand(activeAgentId, { kind, thread_id: id }).catch((e: unknown) =>
        flash(describeCommandError(verb, e)),
      )
    },
    [threads, activeAgentId, flash, selectedId, setSelectedId],
  )

  const handlePause = useCallback(
    (id: string) => {
      const t = threads.find((th) => th.id === id)
      if (!t) return
      const kind = t.paused ? "resume_thread" : "pause_thread"
      const verb = t.paused ? "resume the thread" : "pause the thread"
      sendCommand(activeAgentId, { kind, thread_id: id }).catch((e: unknown) =>
        flash(describeCommandError(verb, e)),
      )
    },
    [threads, activeAgentId, flash],
  )

  const handleDelete = useCallback(
    (id: string) => {
      if (id === selectedId) setSelectedId("")
      sendCommand(activeAgentId, { kind: "delete_thread", thread_id: id }).catch((e: unknown) =>
        flash(describeCommandError("delete the thread", e)),
      )
    },
    [activeAgentId, flash, selectedId, setSelectedId],
  )

  const handleCreate = useCallback(
    (title: string) => {
      sel.armAutoSelect()
      sendCommand(activeAgentId, {
        kind: "create_thread",
        name: title.trim() || "Untitled thread",
      }).catch((e: unknown) => {
        sel.disarmAutoSelect()
        flash(describeCommandError("create the thread", e))
      })
      sel.setNewOpen(false)
      sel.setQuery("")
      sel.setShowArchived(false)
    },
    [activeAgentId, flash, sel],
  )

  const handleSend = useCallback(
    (text: string) => {
      if (!effectiveSelectedId) return
      const content = buildCombinedContent(text, pendingFiles)
      if (!content) return
      sendCommand(activeAgentId, {
        kind: "send_message",
        thread_id: effectiveSelectedId,
        content,
      }).catch((e: unknown) => flash(describeCommandError("send your message", e)))
      setPendingFiles([])
    },
    [activeAgentId, effectiveSelectedId, flash, pendingFiles, setPendingFiles],
  )

  const handleAttach = useCallback(
    (files: File[]) => {
      if (!effectiveSelectedId || files.length === 0) return
      // Return the promise so the conversation's drop handler can `await` the
      // upload and keep its loader up until it lands (T471).
      return (async () => {
        try {
          const uploaded: UploadedFile[] = []
          for (const f of files) {
            const r = await uploadUnique(activeAgentId, ".uploads", f)
            uploaded.push({
              path: r.path,
              name: r.name,
              size: r.size,
              note: `uploaded by user at ${new Date().toISOString()}`,
            })
          }
          setPendingFiles((prev) => [...prev, ...uploaded])
        } catch (e) {
          flash(describeCommandError("upload the file", e))
        }
      })()
    },
    [activeAgentId, effectiveSelectedId, flash, setPendingFiles],
  )

  return {
    notice,
    handleArchive,
    handlePause,
    handleDelete,
    handleCreate,
    handleSend,
    handleAttach,
  }
}

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
}: {
  activeAgentId: string
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: (path: string) => void
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
    <div className="flex min-h-0 flex-1">
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
          className="fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-xl border border-[var(--danger)]/40 bg-card px-4 py-2.5 text-[12.5px] text-foreground/90 card-shadow"
        >
          <AlertTriangle className="size-4 shrink-0 text-[var(--danger)]" />
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
          className="flex items-center gap-2 rounded-lg bg-[var(--signal)] px-3.5 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
        >
          <Plus className="size-4" />
          New Thread
        </button>
      )}
    </div>
  )
}
