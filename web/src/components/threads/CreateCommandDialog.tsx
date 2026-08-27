import { AgentEditorDialog } from "@/components/shell/behaviour/AgentEditorDialog"

/**
 * Create Command dialog (T350 / T654) — authors a new `/command` in the active
 * agent's prompt library, opened by the composer's dashed "create command" pill.
 *
 * This is now a thin wrapper over {@link AgentEditorDialog} with
 * `variant="command"`: the command flow deliberately reuses the *exact same*
 * dialog component/code as the behaviour-agent editor (big borderless name line,
 * `/slug` preview, description, prompt textarea, sheet-pop-in chrome) so the two
 * authoring surfaces can never drift. The command variant is always create-only
 * (`mode={{ kind: "create" }}`) and wires to `POST …/library/command`.
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
