import { AgentEditorDialog } from "@/mobile-components/shell/behaviour/AgentEditorDialog"

/**
 * Mobile Create Command dialog (T350 / T654) — thin wrapper over the shared
 * {@link AgentEditorDialog} with `variant="command"`, mirroring the desktop
 * twin exactly.
 *
 * The command authoring surface deliberately reuses the *same* dialog
 * component/code as the behaviour-agent editor so the two can never drift; the
 * mobile `AgentEditorDialog` is itself a stub re-export of the desktop one, so
 * this wrapper inherits identical chrome on both idioms. Always create-only
 * (`mode={{ kind: "create" }}`), wired to `POST …/library/command`.
 */
export function CreateCommandDialog({
  open,
  onClose,
  agentId,
}: {
  open: boolean
  onClose: () => void
  agentId: string
}) {
  return (
    <AgentEditorDialog
      open={open}
      onClose={onClose}
      agentId={agentId}
      mode={{ kind: "create" }}
      variant="command"
    />
  )
}
