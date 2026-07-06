import { useState, useCallback, useRef, useEffect, useMemo } from "react"
import { FolderGit2, AlertTriangle, Plus } from "lucide-react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { NewThreadDialog } from "./NewThreadDialog"
import { useThreads, useFleet, sendCommand } from "@/lib/live"
import { uploadUnique } from "@/lib/api"
import { buildUploadMessage, type UploadedFile } from "./fileUpload/helpers"

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
  onShowInFinder,
}: {
  activeAgentId: string
  /** navigate the Finder to a file's parent directory and select it (T334) */
  onShowInFinder?: (path: string) => void
}) {
  const { data: agents = [] } = useFleet()
  const { data: threads = [] } = useThreads(activeAgentId)
  const agent = agents.find((a) => a.id === activeAgentId)

  // The selected thread is remembered PER AGENT in localStorage, so switching
  // to the Finder (which unmounts this view — App renders ThreadsView only when
  // the threads view is active) and coming back restores the same thread
  // instead of falling through to the first one (T303). Lazily seeded from the
  // store; kept in sync by the persist effect below. Keyed by agent so each
  // realm remembers its own thread.
  const threadKey = `cp-thread-${activeAgentId}`
  const [selectedId, setSelectedId] = useState(() => localStorage.getItem(threadKey) ?? "")
  const [query, setQuery] = useState("")
  const [showArchived, setShowArchived] = useState(false)
  const [newOpen, setNewOpen] = useState(false)

  // ── Pending file attachments (T331) ────────────────────────────────────────
  // Files picked via the composer paperclip are uploaded eagerly (to `.uploads/`)
  // but NOT sent as a message yet — they queue here so the user can add a text
  // caption before sending. On send, the pending files are merged into the
  // message content alongside the typed text, then cleared.
  const [pendingFiles, setPendingFiles] = useState<UploadedFile[]>([])

  // ── Auto-select a just-created thread ──────────────────────────────────────
  // The thread ID is assigned server-side and arrives via an SSE delta. We set a
  // "pending select" flag on create; on the next `threads` update we diff the
  // ID set and select the newcomer. This closes the UX gap where the user had
  // to manually click the new thread after creating it.
  const pendingSelect = useRef(false)
  const prevThreadIdsRef = useRef<Set<string>>(new Set())

  const currentIds = useMemo(() => new Set(threads.map((t) => t.id)), [threads])
  useEffect(() => {
    if (pendingSelect.current && threads.length > 0) {
      const newId = threads.find((t) => !prevThreadIdsRef.current.has(t.id))?.id
      if (newId) {
        setSelectedId(newId)
        pendingSelect.current = false
      }
    }
    prevThreadIdsRef.current = currentIds
  }, [threads, currentIds])

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
    : (threads.find((t) => !t.archived)?.id ?? threads[0]?.id ?? "")

  const thread = threads.find((t) => t.id === effectiveSelectedId)

  // Persist the RESOLVED selection so a view switch (or a full reload) returns
  // to the same thread (T303). We store the effective id — not the raw
  // selectedId — so the remembered value is always a thread that actually
  // exists (e.g. after an archive deselect resolves to the next thread).
  useEffect(() => {
    if (effectiveSelectedId) localStorage.setItem(threadKey, effectiveSelectedId)
  }, [effectiveSelectedId, threadKey])

  // Clear pending file attachments when switching threads — the files are
  // already persisted on disk (`.uploads/`), but they belong to the thread
  // the user was composing in; carrying them over would be confusing.
  useEffect(() => {
    setPendingFiles([])
  }, [effectiveSelectedId])

  const handleArchive = useCallback(
    (id: string) => {
      const t = threads.find((th) => th.id === id)
      if (!t) return
      const kind = t.archived ? "restore_thread" : "archive_thread"
      const verb = t.archived ? "restore the thread" : "archive the thread"
      // Deselect the thread being archived so the view falls through to the
      // next available thread instead of sticking on a now-invisible row.
      if (!t.archived && id === selectedId) setSelectedId("")
      sendCommand(activeAgentId, { kind, thread_id: id }).catch((e) =>
        flash(describeCommandError(verb, e)),
      )
    },
    [threads, activeAgentId, flash, selectedId],
  )

  const handlePause = useCallback(
    (id: string) => {
      const t = threads.find((th) => th.id === id)
      if (!t) return
      const kind = t.paused ? "resume_thread" : "pause_thread"
      const verb = t.paused ? "resume the thread" : "pause the thread"
      sendCommand(activeAgentId, { kind, thread_id: id }).catch((e) =>
        flash(describeCommandError(verb, e)),
      )
    },
    [threads, activeAgentId, flash],
  )

  const handleDelete = useCallback(
    (id: string) => {
      if (id === selectedId) setSelectedId("")
      sendCommand(activeAgentId, { kind: "delete_thread", thread_id: id }).catch((e) =>
        flash(describeCommandError("delete the thread", e)),
      )
    },
    [activeAgentId, flash, selectedId],
  )

  const handleCreate = useCallback(
    (title: string) => {
      pendingSelect.current = true
      sendCommand(activeAgentId, {
        kind: "create_thread",
        name: title.trim() || "Untitled thread",
      }).catch((e) => {
        pendingSelect.current = false
        flash(describeCommandError("create the thread", e))
      })
      setNewOpen(false)
      setQuery("")
      setShowArchived(false)
    },
    [activeAgentId, flash],
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
      }).catch((e) => flash(describeCommandError("send your message", e)))
      setPendingFiles([])
    },
    [activeAgentId, effectiveSelectedId, flash, pendingFiles],
  )

  /**
   * Upload one or more picked files into this thread's staging area (T331).
   *
   * Each file is stored in the realm's `.uploads/` (dedup-suffixed on collision)
   * and added to `pendingFiles` — visible as chips above the composer. The
   * actual thread message (text + file blocks) is NOT created until the user
   * explicitly presses Send, so they can add a caption first. This avoids
   * flipping the thread to MY_TURN prematurely.
   */
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
    [activeAgentId, effectiveSelectedId, flash],
  )

  // Only bail to a bare empty state when there is genuinely no agent. A fresh
  // agent that simply has zero threads MUST still render the sidebar — that is
  // where the "New Thread" button lives — otherwise the realm is a dead end
  // with no way to create the first thread (the sidebar would never show up).
  if (!agent) {
    return <EmptyRealm agentName={undefined} />
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
        onDelete={handleDelete}
        onPause={handlePause}
        onNewThread={() => setNewOpen(true)}
      />

      {/* The conversation pane shows the selected thread, or — for a realm with
          no thread selected/created yet — a hint pointing at the sidebar's New
          Thread button so an empty realm is never a dead end. */}
      {thread ? (
        <ThreadConversation
          thread={thread}
          agentId={activeAgentId}
          onSend={handleSend}
          onAttach={handleAttach}
          pendingFiles={pendingFiles}
          onRemoveFile={(i) => setPendingFiles((prev) => prev.filter((_, idx) => idx !== i))}
          onShowInFinder={onShowInFinder}
        />
      ) : (
        <EmptyRealm agentName={agent.name} onNewThread={() => setNewOpen(true)} />
      )}

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
