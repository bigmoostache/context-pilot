// ── Optimistic overlay for agent creation + message send ────────────
//
// Critical updates show at once with the shared pending loader, then vanish
// into the real roster row / log message once the backend echoes them. No
// retry: a failed command drops its entry and the caller restores the
// pre-execute state (re-filled dialog, textarea text).

import { useCallback, useMemo, useState } from "react"
import { sendCommand } from "@/lib/live"
import type { AgentDetail } from "@/lib/types"
import {
  buildCombinedContent,
  describeCommandError,
  restoreNewAgentDraft,
  type CreateAgentOpts,
} from "./commands"
import type { Selection } from "./index"

interface PendingAgent {
  tempId: string
  name: string
  /** roster ids at submit time — a NEW id with the same name confirms it */
  knownIds: Set<string>
}

interface PendingMsg {
  id: string
  agentId: string
  content: string
  /** log ids at submit time — a NEW user message with the same text confirms it */
  knownIds: Set<string>
}

const tempId = (prefix: string) =>
  `${prefix}-pending-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`

const agentConfirmed = (p: PendingAgent, agents: AgentDetail[]) =>
  agents.some((a) => a.name === p.name && !p.knownIds.has(a.id))

const msgConfirmed = (p: PendingMsg, agents: AgentDetail[]) =>
  agents
    .find((a) => a.id === p.agentId)
    ?.log.some((m) => m.author === "user" && m.text === p.content && !p.knownIds.has(m.id)) ?? false

/** The optimistic layer: the roster with unconfirmed agents/messages overlaid,
 *  plus add/drop handles for the create and send flows. */
function useOptimistic(agents: AgentDetail[]) {
  const [pendingAgents, setPendingAgents] = useState<PendingAgent[]>([])
  const [pendingMsgs, setPendingMsgs] = useState<PendingMsg[]>([])

  const addAgent = useCallback(
    (name: string) => {
      const p: PendingAgent = {
        tempId: tempId("agent"),
        name,
        knownIds: new Set(agents.map((a) => a.id)),
      }
      // Prune confirmed entries while we are here (no setState in render).
      setPendingAgents((prev) => [...prev.filter((x) => !agentConfirmed(x, agents)), p])
      return p.tempId
    },
    [agents],
  )
  const dropAgent = useCallback((id: string) => {
    setPendingAgents((prev) => prev.filter((x) => x.tempId !== id))
  }, [])

  const addMsg = useCallback(
    (agentId: string, content: string) => {
      const log = agents.find((a) => a.id === agentId)?.log ?? []
      const p: PendingMsg = {
        id: tempId("msg"),
        agentId,
        content,
        knownIds: new Set(log.map((m) => m.id)),
      }
      setPendingMsgs((prev) => [...prev.filter((x) => !msgConfirmed(x, agents)), p])
      return p.id
    },
    [agents],
  )
  const dropMsg = useCallback((id: string) => {
    setPendingMsgs((prev) => prev.filter((x) => x.id !== id))
  }, [])

  const view = useMemo(() => {
    const msgs = pendingMsgs.filter((p) => !msgConfirmed(p, agents))
    const rows = pendingAgents.filter((p) => !agentConfirmed(p, agents))
    if (msgs.length === 0 && rows.length === 0) return agents
    const withMsgs = agents.map((a) => {
      const mine = msgs.filter((p) => p.agentId === a.id)
      if (mine.length === 0) return a
      return {
        ...a,
        log: [
          ...a.log,
          ...mine.map((p) => ({
            id: p.id,
            author: "user" as const,
            text: p.content,
            pending: true,
          })),
        ],
      }
    })
    const fleetId = agents[0]?.fleetId ?? ""
    return [
      ...withMsgs,
      ...rows.map((p): AgentDetail => ({
        id: p.tempId,
        fleetId,
        name: p.name,
        status: "THEIR_TURN",
        log: [],
        depth: 0,
        pending: true,
      })),
    ]
  }, [agents, pendingAgents, pendingMsgs])

  return { view, addAgent, dropAgent, addMsg, dropMsg }
}

/**
 * The create + send handlers, optimistic: the agent row / message bubble shows
 * at once (pending loader) and settles when the backend echoes it. On failure,
 * no retry — the entry vanishes and the pre-execute state comes back (dialog
 * reopened pre-filled; `handleSend` resolves false so the composer restores
 * its text, attachments are handed back).
 */
export function useOptimisticCommands(
  activeFleetId: string,
  agents: AgentDetail[],
  sel: Selection,
  flash: (msg: string) => void,
) {
  const optimistic = useOptimistic(agents)
  const { effectiveSelectedId, pendingFiles, setPendingFiles } = sel

  const handleCreate = useCallback(
    (opts: CreateAgentOpts) => {
      sel.setNewOpen(false)
      sel.setQuery("")
      sel.setShowArchived(false)
      // ONE atomic command carries name + first message + paused (T687): the
      // message rides the durable command payload, so it can never vanish.
      const content = buildCombinedContent(opts.firstMessage, opts.files, true)
      // Visual auto-select of the newcomer once it appears (selection only).
      sel.armAutoSelect()
      const name = opts.title.trim() || "Untitled agent"
      const pendingId = optimistic.addAgent(name)
      void (async () => {
        try {
          await sendCommand(activeFleetId, {
            kind: "create_agent",
            name,
            ...(content && { initial_message: content }),
            paused: opts.paused,
          })
        } catch (e) {
          optimistic.dropAgent(pendingId)
          sel.disarmAutoSelect()
          restoreNewAgentDraft(activeFleetId, opts)
          sel.setNewOpen(true)
          flash(describeCommandError("create the agent", e))
        }
      })()
    },
    [activeFleetId, flash, sel, optimistic],
  )

  const handleSend = useCallback(
    async (text: string): Promise<boolean> => {
      if (!effectiveSelectedId) return true
      const files = pendingFiles
      const content = buildCombinedContent(text, files)
      if (!content) return true
      const pendingId = optimistic.addMsg(effectiveSelectedId, content)
      setPendingFiles([])
      try {
        await sendCommand(activeFleetId, {
          kind: "send_message",
          agent_id: effectiveSelectedId,
          content,
        })
        return true
      } catch (e) {
        optimistic.dropMsg(pendingId)
        setPendingFiles(files)
        flash(describeCommandError("send your message", e))
        return false
      }
    },
    [activeFleetId, effectiveSelectedId, flash, pendingFiles, setPendingFiles, optimistic],
  )

  return { view: optimistic.view, handleCreate, handleSend }
}
