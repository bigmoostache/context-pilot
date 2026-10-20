// ── Agent view orchestration logic (shared, non-forked) ─────────────
//
// The selection + mutation logic that drives the agent surface, extracted out
// of `components/agents/AgentsView.tsx` so the desktop master-detail view AND
// the mobile stack-navigation view consume the EXACT same behaviour — only the
// presentation forks (design-mobile.md §3.2, architecture rule M141: no logic
// duplication across component trees).
//
// Two hooks: `useAgentSelection` (which agent is active + composer-adjacent
// view state, persisted per agent) and `useAgentActions` (every archive /
// pause / delete / create / send / attach command dispatch, each surfacing its
// failure as a visible notice). Both are pure orchestration over the shared
// `@/lib/live` hooks — zero component imports, so importing this from either
// tree never crosses the mirror leak-guard.

import { useState, useCallback, useRef, useEffect, useMemo } from "react"
import { sendCommand } from "@/lib/live"
import { uploadUnique } from "@/lib/api"
import type { UploadedFile } from "@/lib/live/agentUpload"
import type { AgentDetail } from "@/lib/types"
import { describeCommandError, type CreateAgentOpts } from "./commands"
import { useAgentBranch, type BranchActions } from "./branch"
import { useOptimisticCommands } from "./optimistic"

export {
  buildCombinedContent,
  describeCommandError,
  newAgentDraftKey,
  type CreateAgentOpts,
} from "./commands"
export type { BranchTarget } from "./branch"
export { useAgentReadState } from "./readState"

/** The agent-selection surface owned by {@link useAgentSelection}. */
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
  /** the resolved-to-a-real-agent id (selection may point at a stale/archived row) */
  effectiveSelectedId: string
  /** the id of a agent that was JUST auto-selected after `armAutoSelect` — the
   *  seam {@link useAgentActions} uses to send a create's first message once the
   *  server-assigned id is known (T679). Null until such an event fires. */
  justCreatedId: string | null
  /** flag the next agents update to auto-select the newly-created agent */
  armAutoSelect: () => void
  /** cancel a pending auto-select (create failed before the id arrived) */
  disarmAutoSelect: () => void
}

/**
 * Own the agent selection + composer-adjacent view state for a realm.
 *
 * Selection is remembered PER AGENT in localStorage so a view switch (Finder ↔
 * agents) or a reload returns to the same agent (T303); the persisted value
 * is always the *effective* (resolved-to-existing) id. A just-created agent is
 * auto-selected once its server-assigned id arrives via the next SSE delta
 * (`armAutoSelect` sets the flag; the diff effect picks the newcomer). Switching
 * agents clears staged uploads via a render-phase reset (React's documented
 * adjust-state-on-prop-change pattern — not an effect, which would cost an extra
 * commit and trip set-state-in-effect).
 *
 * @param newAgentDialog Optional caller-owned open flag for the New Agent
 *                        dialog. Supply it when the trigger lives OUTSIDE the
 *                        agent view (the desktop header rail); omit it to keep
 *                        the flag local (mobile).
 * @param controlledSelection Optional caller-owned selection (id + setter).
 *                        Supply it when an ancestor must OWN which agent is
 *                        active — the desktop shell does, so browser Back/Next
 *                        can step through the agents visited (T636). When
 *                        given, this hook stops seeding/persisting the
 *                        selection itself (the owner does both); when omitted
 *                        (mobile), it keeps its own localStorage-backed state.
 */
export function useAgentSelection(
  activeFleetId: string,
  agents: AgentDetail[],
  newAgentDialog?: { open: boolean; setOpen: (v: boolean) => void },
  controlledSelection?: { selectedId: string; setSelectedId: (id: string) => void },
): Selection {
  const agentKey = `cp-agent-sel-${activeFleetId}`
  // Internal fallback state — used ONLY when the caller does not control the
  // selection (mobile). A controlled caller owns the value + its persistence,
  // so this state (and the seed/persist effects below) sit inert.
  const [ownSelectedId, setOwnSelectedId] = useState(() => localStorage.getItem(agentKey) ?? "")
  const controlled = controlledSelection !== undefined
  const selectedId = controlled ? controlledSelection.selectedId : ownSelectedId
  const setSelectedId = controlled ? controlledSelection.setSelectedId : setOwnSelectedId
  const [query, setQuery] = useState("")
  const [showArchived, setShowArchived] = useState(false)
  const [pendingFiles, setPendingFiles] = useState<UploadedFile[]>([])

  // The new-agent dialog's open flag, optionally OWNED BY THE CALLER.
  //
  // The desktop shell moved the "New agent" button into the header rail, which
  // is a SIBLING of the agents view rather than a descendant — so the flag has
  // to live above both, and the caller hands it in here. Injecting it (rather
  // than letting the caller bypass `sel.newOpen`) is what keeps `handleCreate`
  // working: that closes the dialog through `sel.setNewOpen(false)`, and it must
  // close the same flag the button opened.
  //
  // Omitting the argument keeps the state local, which is what the mobile tree
  // does — its own trigger sits inside the view.
  const [ownNewOpen, setOwnNewOpen] = useState(false)
  const newOpen = newAgentDialog?.open ?? ownNewOpen
  const setNewOpen = newAgentDialog?.setOpen ?? setOwnNewOpen

  // Auto-select a just-created agent once its server-assigned id lands.
  const pendingSelectRef = useRef(false)
  const prevAgentIdsRef = useRef<Set<string>>(new Set())
  const currentIds = useMemo(() => new Set(agents.map((t) => t.id)), [agents])
  const [justCreatedId, setJustCreatedId] = useState<string | null>(null)
  useEffect(() => {
    if (pendingSelectRef.current && agents.length > 0) {
      const newId = agents.find((t) => !prevAgentIdsRef.current.has(t.id))?.id
      if (newId) {
        setSelectedId(newId)
        setJustCreatedId(newId)
        pendingSelectRef.current = false
      }
    }
    prevAgentIdsRef.current = currentIds
  }, [agents, currentIds, setSelectedId])

  // Resolve the selection to a agent that actually exists, falling back to the
  // first non-archived (then any) agent.
  const validSelection = agents.some((t) => t.id === selectedId)
  const effectiveSelectedId = validSelection
    ? selectedId
    : (agents.find((t) => !t.archived)?.id ?? agents[0]?.id ?? "")

  // Persist the RESOLVED selection so a reload returns to a still-existing
  // agent — but ONLY when uncontrolled. A controlled owner (the desktop shell)
  // persists per-agent itself, so writing here too would be a second writer to
  // the same key.
  useEffect(() => {
    if (!controlled && effectiveSelectedId) localStorage.setItem(agentKey, effectiveSelectedId)
  }, [controlled, effectiveSelectedId, agentKey])

  // Clear staged uploads when the agent changes (render-phase reset).
  const [pendingAgent, setPendingAgent] = useState(effectiveSelectedId)
  if (pendingAgent !== effectiveSelectedId) {
    setPendingAgent(effectiveSelectedId)
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

/** The command handlers + transient notice returned by {@link useAgentActions}. */
export interface Actions extends BranchActions {
  notice: Notice | null
  /** the roster with optimistic (pending) agents + messages overlaid — render this */
  agents: AgentDetail[]
  handleArchive: (id: string) => void
  handlePause: (id: string) => void
  /** rename an agent; the `agent_renamed` delta updates the list */
  handleRename: (id: string, name: string) => void
  handleDelete: (id: string) => void
  handleCreate: (opts: CreateAgentOpts) => void
  /** resolves false when the send failed, so the composer restores its text */
  handleSend: (text: string) => Promise<boolean>
  handleAttach: (files: File[]) => void | Promise<void>
}

/**
 * All agent-mutation handlers for a realm, plus a transient failure notice.
 *
 * A command rejected by the backend must be *visible*, never a silent
 * `.catch(console.error)` swallow (T121): every handler routes its rejection
 * through {@link describeCommandError} into `flash`, which shows one
 * auto-dismissing toast at a time (cleared on unmount so a late tick can't
 * setState a dead component).
 */
export function useAgentActions(
  activeFleetId: string,
  agents: AgentDetail[],
  sel: Selection,
): Actions {
  const { selectedId, setSelectedId, effectiveSelectedId, setPendingFiles } = sel
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
          sendCommand(activeFleetId, { kind: inverseKind, agent_id: id }).catch((e: unknown) =>
            flash(describeCommandError(inverseVerb, e)),
          )
        },
      })
    },
    [activeFleetId, show, flash],
  )
  useEffect(
    () => () => {
      if (noticeTimerRef.current !== null) window.clearTimeout(noticeTimerRef.current)
    },
    [],
  )

  const handleArchive = useCallback(
    (id: string) => {
      const t = agents.find((th) => th.id === id)
      if (!t) return
      const kind = t.archived ? "restore_agent" : "archive_agent"
      const verb = t.archived ? "restore the agent" : "archive the agent"
      // Deselect the archived agent so the view falls through to the next one.
      if (!t.archived && id === selectedId) setSelectedId("")
      sendCommand(activeFleetId, { kind, agent_id: id })
        .then(() =>
          // The action's own inverse, offered as an undo (T636). Archiving is
          // undone by restoring and vice-versa.
          t.archived
            ? offerUndo("Agent restored", id, "archive_agent", "re-archive the agent")
            : offerUndo("Agent archived", id, "restore_agent", "restore the agent"),
        )
        .catch((e: unknown) => flash(describeCommandError(verb, e)))
    },
    [agents, activeFleetId, flash, offerUndo, selectedId, setSelectedId],
  )

  const handlePause = useCallback(
    (id: string) => {
      const t = agents.find((th) => th.id === id)
      if (!t) return
      const kind = t.paused ? "resume_agent" : "pause_agent"
      const verb = t.paused ? "resume the agent" : "pause the agent"
      sendCommand(activeFleetId, { kind, agent_id: id })
        .then(() =>
          t.paused
            ? offerUndo("Agent resumed", id, "pause_agent", "re-pause the agent")
            : offerUndo("Agent paused", id, "resume_agent", "resume the agent"),
        )
        .catch((e: unknown) => flash(describeCommandError(verb, e)))
    },
    [agents, activeFleetId, flash, offerUndo],
  )

  const handleRename = useCallback(
    (id: string, name: string) => {
      sendCommand(activeFleetId, { kind: "rename_agent", agent_id: id, name }).catch((e: unknown) =>
        flash(describeCommandError("rename the agent", e)),
      )
    },
    [activeFleetId, flash],
  )

  const handleDelete = useCallback(
    (id: string) => {
      if (id === selectedId) setSelectedId("")
      sendCommand(activeFleetId, { kind: "delete_agent", agent_id: id }).catch((e: unknown) =>
        flash(describeCommandError("delete the agent", e)),
      )
    },
    [activeFleetId, flash, selectedId, setSelectedId],
  )

  const { view, handleCreate, handleSend } = useOptimisticCommands(
    activeFleetId,
    agents,
    sel,
    flash,
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
            const r = await uploadUnique(activeFleetId, ".uploads", f)
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
    [activeFleetId, effectiveSelectedId, flash, setPendingFiles],
  )

  const branch = useAgentBranch(activeFleetId, sel, flash)

  return {
    ...branch,
    notice,
    agents: view,
    handleArchive,
    handlePause,
    handleRename,
    handleDelete,
    handleCreate,
    handleSend,
    handleAttach,
  }
}
