import { SystemEditorDialog } from "@/mobile-components/shell/behaviour/SystemEditor"

/**
 * Mobile Create Command dialog (T350 / T654) — thin wrapper over the shared
 * {@link SystemEditorDialog} with `variant="command"`, mirroring the desktop
 * twin exactly.
 *
 * The command authoring surface deliberately reuses the *same* dialog
 * component/code as the behaviour-agent editor so the two can never drift; the
 * mobile `SystemEditorDialog` is itself a stub re-export of the desktop one, so
 * this wrapper inherits identical chrome on both idioms. Always create-only
 * (`mode={{ kind: "create" }}`), wired to `POST …/library/command`.
 */
export function CreateCommandDialog({
  open,
  onClose,
  fleetId,
}: {
  open: boolean
  onClose: () => void
  fleetId: string
}) {
  return (
    <SystemEditorDialog
      open={open}
      onClose={onClose}
      fleetId={fleetId}
      mode={{ kind: "create" }}
      variant="command"
    />
  )
}
