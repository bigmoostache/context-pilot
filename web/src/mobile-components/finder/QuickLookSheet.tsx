import { Sheet, SheetContent } from "@/mobile-components/ui/sheet"
import { FinderPreview } from "./preview/FinderPreview"
import type { FinderNode } from "@/lib/types"

/**
 * The mobile Finder Quick Look drawer — the touch twin of the desktop
 * right-side Sheet. On a phone a full-height side rail is unusable, so this
 * slides up from the BOTTOM at ~90% of the viewport height (the iOS Files /
 * Quick Look idiom) and renders a {@link FinderPreview} of one node. The Sheet
 * primitive still brings the dimming backdrop, slide animation, focus trap,
 * scroll lock, and Esc + tap-outside dismissal; its built-in close button is
 * hidden because the preview pane draws its own Quick Look header with a Close
 * control.
 *
 * This is the single mobile drawer implementation, shared by the mobile Finder
 * (its grid/list Quick Look) and the mobile threads conversation (a
 * `file-upload` chip opens the same drawer) — the two call sites must never
 * drift into two copies of the preview drawer.
 */
export function QuickLookSheet({
  node,
  agentId,
  open,
  onClose,
}: {
  node: FinderNode | null
  agentId: string
  open: boolean
  onClose: () => void
}) {
  return (
    <Sheet
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
    >
      <SheetContent
        side="bottom"
        showCloseButton={false}
        className="h-[90vh] rounded-t-2xl border-t border-border p-0"
      >
        <FinderPreview node={node} agentId={agentId} onClose={onClose} />
      </SheetContent>
    </Sheet>
  )
}
