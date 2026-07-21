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

/**
 * Build a combined message body from user text and pending file attachments.
 * Text comes first (if any), then file blocks. Either can be absent — a
 * send with only pending files produces just the file blocks; one with only
 * text produces just text.
 */
export function buildCombinedContent(text: string, files: UploadedFile[]): string {
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
export function useThreadSelection(activeAgentId: string, threads: ThreadDetail[]): Selection {
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
export interface Actions {
  notice: string | null
  handleArchive: (id: string) => void
  handlePause: (id: string) => void
  handleDelete: (id: string) => void
  handleCreate: (title: string) => void
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

