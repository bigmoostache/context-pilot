import { useEffect, useRef, useState } from "react"
import type { Agent } from "@/lib/types"
import {
  useCreateAgent,
  useRenameAgent,
  useRestartAgent,
  useRetireAgent,
  useUploadAvatar,
  sendCommand,
} from "@/lib/live"
import { findModel, type ProviderDef } from "@/lib/support/models"
import { slugify, type SelectionState } from "./controller"

/** The mutation surface + derived busy/error flags returned by
 *  {@link useAgentModalActions}. */
export interface Actions {
  error: string | null
  saving: boolean
  pending: boolean
  submit: () => void
  retire: () => void
  restart: () => void
  retireBusy: boolean
  restartBusy: boolean
  avatarBust: number
  onAvatarChange: (file: File) => void
}

/** Inputs to {@link useAgentModalActions} — bundled into one object so the hook
 *  stays within the max-params budget. */
export interface ActionsArgs {
  isManage: boolean
  agent: Agent | undefined
  name: string
  sel: SelectionState
  providers: ProviderDef[]
  onClose: () => void
  onFlash: ((m: string) => void) | undefined
}

/**
 * All create/rename/restart/retire/avatar mutations plus the Esc/⌘↵ key handler,
 * extracted from the controller so each unit meets the P8 budgets. `submit`
 * dispatches to create or save-manage; the key handler binds ONCE on mount and
 * reads `submit`/`onClose` through latest-refs kept fresh by the assignment
 * effect (so the mount-only binding sees live values without re-binding and
 * without an inline eslint-disable, P4-banned).
 */
export function useAgentModalActions(args: ActionsArgs): Actions {
  const { isManage, agent, name, sel, providers, onClose, onFlash } = args
  const createAgent = useCreateAgent()
  const restartAgent = useRestartAgent()
  const retireAgent = useRetireAgent()
  const renameAgent = useRenameAgent()
  const uploadAvatar = useUploadAvatar()
  const [avatarBust, setAvatarBust] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const pending = createAgent.isPending || saving || restartAgent.isPending || retireAgent.isPending

  const onAvatarChange = (file: File) => {
    if (!agent) return
    uploadAvatar.mutate(
      { agentId: agent.id, file },
      {
        onSuccess: () => setAvatarBust(Date.now()),
        onError: (err) => setError(err instanceof Error ? err.message : "Avatar upload failed"),
      },
    )
  }

  /** Restart a (possibly stale-binary) agent so a fresh process can accept
   *  commands the old binary rejected with `502 agent unreachable`. */
  const restart = () => {
    if (!agent || restartAgent.isPending) return
    setError(null)
    restartAgent.mutate(agent.id, {
      onSuccess: () => {
        onFlash?.(`Restarting ${agent.name} — it will reconnect in a moment`)
        onClose()
      },
      onError: (e) => setError(e instanceof Error ? e.message : "Could not restart the agent."),
    })
  }

  /** Retire (archive) the agent: stop its process + console server, keep its
   *  folder, and move it to the dashboard's Retired section. Reversible. */
  const retire = () => {
    if (!agent || retireAgent.isPending) return
    setError(null)
    retireAgent.mutate(agent.id, {
      onSuccess: () => {
        onFlash?.(`Retired ${agent.name} — moved to the Retired section`)
        onClose()
      },
      onError: (e) => setError(e instanceof Error ? e.message : "Could not retire the agent."),
    })
  }

  const saveManage = (a: Agent) => {
    setSaving(true)
    setError(null)
    const nameChanged = name.trim() !== a.name
    const tasks: Promise<unknown>[] = [
      sendCommand(a.id, { kind: "configure", provider: sel.provId, model: sel.modelId }),
    ]
    if (nameChanged) tasks.push(renameAgent.mutateAsync({ agentId: a.id, name: name.trim() }))
    Promise.all(tasks)
      .then(() => {
        onFlash?.(
          nameChanged
            ? `Saved changes to ${name.trim()}`
            : `Model updated to ${findModel(providers, sel.provId, sel.modelId)?.displayName ?? sel.modelId}`,
        )
        onClose()
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "Failed to save changes"))
      .finally(() => setSaving(false))
  }

  const create = () => {
    setError(null)
    const apiName = findModel(providers, sel.provId, sel.modelId)?.apiName
    createAgent.mutate(
      { name: name.trim(), ...(apiName && { model: apiName }) },
      {
        onSuccess: (receipt) => {
          onFlash?.(`Spawning “${slugify(name)}” in ${receipt.folder}`)
          onClose()
        },
        onError: (e) =>
          setError(
            e instanceof Error ? e.message : "Could not create the agent. Please try again.",
          ),
      },
    )
  }

  const canSubmit = (isManage || name.trim().length > 0) && !pending
  const submit = () => {
    if (!canSubmit) return
    if (isManage && agent) saveManage(agent)
    else create()
  }

  // Esc closes, ⌘/Ctrl+Enter submits — bound once on mount via latest-refs.
  const submitRef = useRef(submit)
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    submitRef.current = submit
    onCloseRef.current = onClose
  })
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCloseRef.current()
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submitRef.current()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])

  return {
    error,
    saving,
    pending,
    submit,
    retire,
    restart,
    retireBusy: retireAgent.isPending,
    restartBusy: restartAgent.isPending,
    avatarBust,
    onAvatarChange,
  }
}
