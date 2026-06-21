import { Sheet, SheetContent } from "@/components/ui/sheet"
import { FinderPreview } from "./preview/FinderPreview"
import type { FinderNode } from "@/lib/types"

/**
 * The Finder Quick Look drawer — a standard modal shadcn {@link Sheet} that
 * slides in from the right at two-thirds of the viewport and renders a
 * {@link FinderPreview} of one node. The component brings the dimming backdrop,
 * slide animation, focus trap, scroll lock, and Esc + click-outside dismissal
 * for free; its built-in close button is hidden because the preview pane draws
 * its own Quick Look header with a Close control.
 *
 * This is the **single** drawer implementation, shared by:
 *   - the {@link Finder} (its grid/list Quick Look), and
 *   - the threads conversation (a `file-upload` attachment chip opens the exact
 *     same drawer for the referenced file).
 *
 * Keeping it in one component is deliberate — the two call sites must never
 * drift into two maintained copies of the preview drawer.
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
        side="right"
        showCloseButton={false}
        className="border-l border-border p-0 data-[side=right]:w-2/3 data-[side=right]:max-w-none data-[side=right]:sm:max-w-none"
      >
        <FinderPreview node={node} agentId={agentId} onClose={onClose} />
      </SheetContent>
    </Sheet>
  )
}
