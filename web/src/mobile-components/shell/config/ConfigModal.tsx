import { Dialog, DialogContent } from "@/mobile-components/ui/dialog"
import { ConfigPanel } from "./ConfigPanel"

/**
 * Settings dialog — mobile twin of `components/shell/config/ConfigModal`. Wraps
 * the shared {@link ConfigPanel} in a portaled Dialog.
 *
 * The desktop twin is a centered `88vh × 1100px` window; on a phone that's the
 * wrong shape, so the mobile sheet is **full-screen** (`w-screen h-screen`, no
 * rounded gutters or max-width) — the config surface owns the whole viewport,
 * which is how a settings screen reads on mobile. The portal still escapes the
 * TopBar's `.vibrancy` `backdrop-filter` containing block and brings the
 * focus-trap / scroll-lock / Esc behaviour.
 *
 * The `ConfigPanel` it renders is itself the mobile twin (relative import), so
 * the whole settings subtree resolves through `@/mobile-components`.
 */
export function ConfigModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex h-screen max-h-screen w-screen max-w-none rounded-none border-0 p-0">
        <ConfigPanel variant="dialog" />
      </DialogContent>
    </Dialog>
  )
}
