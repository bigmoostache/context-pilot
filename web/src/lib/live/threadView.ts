// ── Thread view orchestration logic (shared, non-forked) ─────────────
//
// The selection + mutation logic that drives the thread surface, extracted out
// of `components/threads/ThreadsView.tsx` so the desktop master-detail view AND
// the mobile stack-navigation view consume the EXACT same behaviour — only the
// presentation forks (design-mobile.md §3.2, architecture rule M141: no logic
// duplication across component trees).
//
// Two hooks: `useThreadSelection` (which thread is active + composer-adjacent
// view state, persisted per agent) and `useThreadActions` (every archive /
// pause / delete / create / send / attach command dispatch, each surfacing its
// failure as a visible notice). Both are pure orchestration over the shared
// `@/lib/live` hooks — zero component imports, so importing this from either
// tree never crosses the mirror leak-guard.

import { useState, useCallback, useRef, useEffect, useMemo } from "react"
import { sendCommand } from "@/lib/live"
import { uploadUnique } from "@/lib/api"
import { buildUploadMessage, type UploadedFile } from "@/lib/live/threadUpload"
import type { ThreadDetail } from "@/lib/types"

/** Rich thread-creation payload collected by the New Thread dialog (T674):
 *  a title plus an optional first message (auto-sent), file attachments, and a
 *  "create paused" flag that queues the seeded message without waking the agent. */
export interface CreateThreadOpts {
  title: string
  /** first user message, auto-sent to the new thread once its id is known (empty = none) */
  firstMessage: string
  /** files ALREADY uploaded to `.uploads/` (the dialog uploads on attach so the
   *  draft — paths included — survives a close/reopen via localStorage); folded
   *  into the first message as `file-upload` blocks at send time */
  files: UploadedFile[]
  /** create the thread already paused (seeded message queued, no MY_TURN nudge) */
  paused: boolean
}

/**
 * Build a combined message body from user text and pending file attachments,
 * reusing the exact same ` ```file-upload ` block composer the thread composer
 * uses ({@link buildUploadMessage}). Either part can be absent — a send with
 * only files produces just the file blocks; one with only text produces just
 * text.
 *
 * `filesFirst` controls ordering. The thread composer sends text first then the
 * file blocks (default, `false`). The new-thread create flow prepends the file
 * blocks so the attachments lead the very first message (T687).
 */
export function buildCombinedContent(
  text: string,
  files: UploadedFile[],
  filesFirst = false,
): string {
  const textPart = text.trim()
  const filePart = files.length > 0 ? buildUploadMessage(files) : ""
  const parts = filesFirst ? [filePart, textPart] : [textPart, filePart]
  return parts.filter(Boolean).join("\n\n")
}

/**
 * Turn a rejected `sendCommand` into a human sentence for the notice toast.
 *
 * Every failure is surfaced visibly so a command is never silently dropped.
 */
export function describeCommandError(verb: string, err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err)
  return `Could not ${verb}: ${msg}`
}

/** The thread-selection surface owned by {@link useThreadSelection}. */
export interface Selection {
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
  /** the id of a thread that was JUST auto-selected after `armAutoSelect` — the
   *  seam {@link useThreadActions} uses to send a create's first message once the
   *  server-assigned id is known (T679). Null until such an event fires. */
  justCreatedId: string | null
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
 *
 * @param newThreadDialog Optional caller-owned open flag for the New Thread
 *                        dialog. Supply it when the trigger lives OUTSIDE the
 *                        thread view (the desktop header rail); omit it to keep
 *                        the flag local (mobile).
 * @param controlledSelection Optional caller-owned selection (id + setter).
 *                        Supply it when an ancestor must OWN which thread is
 *                        active — the desktop shell does, so browser Back/Next
 *                        can step through the threads visited (T636). When
 *                        given, this hook stops seeding/persisting the
 *                        selection itself (the owner does both); when omitted
 *                        (mobile), it keeps its own localStorage-backed state.
 */
export function useThreadSelection(
  activeAgentId: string,
  threads: ThreadDetail[],
  newThreadDialog?: { open: boolean; setOpen: (v: boolean) => void },
  controlledSelection?: { selectedId: string; setSelectedId: (id: string) => void },
): Selection {
  const threadKey = `cp-thread-${activeAgentId}`
  // Internal fallback state — used ONLY when the caller does not control the
  // selection (mobile). A controlled caller owns the value + its persistence,
  // so this state (and the seed/persist effects below) sit inert.
  const [ownSelectedId, setOwnSelectedId] = useState(() => localStorage.getItem(threadKey) ?? "")
  const controlled = controlledSelection !== undefined
  const selectedId = controlled ? controlledSelection.selectedId : ownSelectedId
  const setSelectedId = controlled ? controlledSelection.setSelectedId : setOwnSelectedId
  const [query, setQuery] = useState("")
  const [showArchived, setShowArchived] = useState(false)
  const [pendingFiles, setPendingFiles] = useState<UploadedFile[]>([])

  // The new-thread dialog's open flag, optionally OWNED BY THE CALLER.
  //
  // The desktop shell moved the "New thread" button into the header rail, which
  // is a SIBLING of the threads view rather than a descendant — so the flag has
  // to live above both, and the caller hands it in here. Injecting it (rather
  // than letting the caller bypass `sel.newOpen`) is what keeps `handleCreate`
  // working: that closes the dialog through `sel.setNewOpen(false)`, and it must
  // close the same flag the button opened.
  //
  // Omitting the argument keeps the state local, which is what the mobile tree
  // does — its own trigger sits inside the view.
  const [ownNewOpen, setOwnNewOpen] = useState(false)
  const newOpen = newThreadDialog?.open ?? ownNewOpen
  const setNewOpen = newThreadDialog?.setOpen ?? setOwnNewOpen

  // Auto-select a just-created thread once its server-assigned id lands.
  const pendingSelectRef = useRef(false)
  const prevThreadIdsRef = useRef<Set<string>>(new Set())
  const currentIds = useMemo(() => new Set(threads.map((t) => t.id)), [threads])
  const [justCreatedId, setJustCreatedId] = useState<string | null>(null)
  useEffect(() => {
    if (pendingSelectRef.current && threads.length > 0) {
      const newId = threads.find((t) => !prevThreadIdsRef.current.has(t.id))?.id
      if (newId) {
        setSelectedId(newId)
        setJustCreatedId(newId)
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

  // Persist the RESOLVED selection so a reload returns to a still-existing
  // thread — but ONLY when uncontrolled. A controlled owner (the desktop shell)
  // persists per-agent itself, so writing here too would be a second writer to
  // the same key.
  useEffect(() => {
    if (!controlled && effectiveSelectedId) localStorage.setItem(threadKey, effectiveSelectedId)
  }, [controlled, effectiveSelectedId, threadKey])

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
    justCreatedId,
    armAutoSelect: () => {
      pendingSelectRef.current = true
    },
    disarmAutoSelect: () => {
      pendingSelectRef.current = false
    },
  }
}

/**
 * A transient toast: an error banner, or a Gmail-style **undo** prompt after a
 * reversible action (T636). `tone` picks the styling (danger vs neutral); when
 * `undo` is present the view renders an Undo button that re-issues the inverse
 * backend command. Kept in this shared hook (not a component) so desktop and
 * mobile drive the exact same undo behaviour off one source.
 */
export interface Notice {
  message: string
  /** Present on an undoable action — runs the inverse command and dismisses. */
  undo?: (() => void) | undefined
  tone: "info" | "error"
}

/** The command handlers + transient notice returned by {@link useThreadActions}. */
export interface Actions {
  notice: Notice | null
  handleArchive: (id: string) => void
  handlePause: (id: string) => void
  handleDelete: (id: string) => void
  handleCreate: (opts: CreateThreadOpts) => void
  handleSend: (text: string) => void
  handleAttach: (files: File[]) => void | Promise<void>
}

/**
 * All thread-mutation handlers for a realm, plus a transient failure notice.
 *
 * A command rejected by the backend must be *visible*, never a silent
 * `.catch(console.error)` swallow (T121): every handler routes its rejection
 * through {@link describeCommandError} into `flash`, which shows one
 * auto-dismissing toast at a time (cleared on unmount so a late tick can't
 * setState a dead component).
 */
export function useThreadActions(
  activeAgentId: string,
  threads: ThreadDetail[],
  sel: Selection,
): Actions {
  const { selectedId, setSelectedId, effectiveSelectedId, pendingFiles, setPendingFiles } = sel

  const [notice, setNotice] = useState<Notice | null>(null)
  const noticeTimerRef = useRef<number | null>(null)
  // Show a toast, replacing any current one, and auto-dismiss it. An undo
  // prompt lingers a touch longer than an error (the user has to decide to act
  // on it), matching the Gmail dwell.
  const show = useCallback((n: Notice) => {
    if (noticeTimerRef.current !== null) window.clearTimeout(noticeTimerRef.current)
    setNotice(n)
    noticeTimerRef.current = window.setTimeout(() => setNotice(null), n.undo ? 7000 : 6000)
  }, [])
  const flash = useCallback((msg: string) => show({ message: msg, tone: "error" }), [show])
  /**
   * Push a Gmail-style undo toast for a reversible action. `inverseKind` is the
   * command that reverses it (restore↔archive, resume↔pause); the Undo button
   * re-issues it through the backend — no local state is rewound, so the
   * backend stays the single source of truth (M141). A failed inverse surfaces
   * its own error toast.
   */
  const offerUndo = useCallback(
    (message: string, id: string, inverseKind: string, inverseVerb: string) => {
      show({
        message,
        tone: "info",
        undo: () => {
          if (noticeTimerRef.current !== null) window.clearTimeout(noticeTimerRef.current)
          setNotice(null)
          sendCommand(activeAgentId, { kind: inverseKind, thread_id: id }).catch((e: unknown) =>
            flash(describeCommandError(inverseVerb, e)),
          )
        },
      })
    },
    [activeAgentId, show, flash],
  )
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
      sendCommand(activeAgentId, { kind, thread_id: id })
        .then(() =>
          // The action's own inverse, offered as an undo (T636). Archiving is
          // undone by restoring and vice-versa.
          t.archived
            ? offerUndo("Thread restored", id, "archive_thread", "re-archive the thread")
            : offerUndo("Thread archived", id, "restore_thread", "restore the thread"),
        )
        .catch((e: unknown) => flash(describeCommandError(verb, e)))
    },
    [threads, activeAgentId, flash, offerUndo, selectedId, setSelectedId],
  )

  const handlePause = useCallback(
    (id: string) => {
      const t = threads.find((th) => th.id === id)
      if (!t) return
      const kind = t.paused ? "resume_thread" : "pause_thread"
      const verb = t.paused ? "resume the thread" : "pause the thread"
      sendCommand(activeAgentId, { kind, thread_id: id })
        .then(() =>
          t.paused
            ? offerUndo("Thread resumed", id, "pause_thread", "re-pause the thread")
            : offerUndo("Thread paused", id, "resume_thread", "resume the thread"),
        )
        .catch((e: unknown) => flash(describeCommandError(verb, e)))
    },
    [threads, activeAgentId, flash, offerUndo],
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
    (opts: CreateThreadOpts) => {
      // Close the dialog + reset the browse state immediately; the create is
      // fire-and-forget with its own failure notice (T121).
      sel.setNewOpen(false)
      sel.setQuery("")
      sel.setShowArchived(false)
      // ONE atomic command carries the whole intent — name, first message, and
      // paused — applied in-process by the agent in strict create -> pause ->
      // send order (T687). This deliberately replaces the former frontend
      // orchestration (create name-only, then guess the new thread's id from a
      // roster set-diff, then fire pause / send from a volatile ref). That
      // orchestration LOST DATA: the first message lived only in a ref, and any
      // derailment — a mis-guessed id, a coalesced/missed roster delta, a
      // second create clobbering the ref, or a pause that never reflected —
      // dropped the message silently. Here the message content rides the
      // durable, deduped command payload and the agent applies it atomically,
      // so it can never vanish. Same upload-on-attach + `file-upload` block
      // path as the composer (buildCombinedContent -> buildUploadMessage);
      // `filesFirst` prepends the blocks so attachments lead the message.
      const content = buildCombinedContent(opts.firstMessage, opts.files, true)
      // Arm the visual auto-select so the new thread opens once it appears
      // (selection only — it no longer gates message delivery, so a mis-select
      // is a harmless view glitch, never data loss).
      sel.armAutoSelect()
      void (async () => {
        try {
          await sendCommand(activeAgentId, {
            kind: "create_thread",
            name: opts.title.trim() || "Untitled thread",
            ...(content && { initial_message: content }),
            paused: opts.paused,
          })
        } catch (e) {
          sel.disarmAutoSelect()
          flash(describeCommandError("create the thread", e))
        }
      })()
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
