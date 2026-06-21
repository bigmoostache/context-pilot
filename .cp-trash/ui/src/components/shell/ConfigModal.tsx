import { Dialog, DialogContent } from "@/components/ui/dialog"
import { ConfigPanel } from "./ConfigPanel"

/**
 * Settings dialog — wraps the shared {@link ConfigPanel} in a portaled Base UI
 * Dialog. Used by the TopBar gear (inside an agent), where settings belong in a
 * modal overlay rather than a full page.
 *
 * The portal is what matters here: it renders into `document.body`, escaping
 * the TopBar's `.vibrancy` `backdrop-filter`, which establishes a containing
 * block that previously trapped a `fixed` overlay inside the 48px header (the
 * "half off-screen / behind content" bug). It also brings focus trapping,
 * scroll-lock and Esc-to-close.
 *
 * The fleet dashboard renders {@link ConfigPanel} inline instead (its own
 * "Settings" page), so the two entry points share one body.
 */
export function ConfigModal({
  open,
  onClose,
}: {
  open: boolean
  onClose: () => void
}) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex h-[88vh] max-h-[860px] w-[1100px] max-w-[calc(100vw-3rem)]">
        <ConfigPanel variant="dialog" />
      </DialogContent>
    </Dialog>
  )
}
