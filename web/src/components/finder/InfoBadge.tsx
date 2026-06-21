import { Info } from "lucide-react"

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"

/**
 * The Finder's per-node **info badge** — a small ⓘ affordance shown on a file or
 * folder that has a tree description (T289). HOVER (or keyboard focus) reveals
 * the description in a tooltip; there is no click behaviour, and the badge keeps
 * the normal cursor (a Tooltip, not a Popover — the right primitive for a
 * hover-only hint: it never opens on click and adds no `help` cursor).
 *
 * The trigger renders as a `<span>` (Base UI `render` prop) so it can live
 * INSIDE a Finder row/cell `<button>` without nesting a button in a button. All
 * pointer events are stopped from propagating so brushing the badge never
 * selects, opens, or drags the underlying node.
 */
export function InfoBadge({ description }: { description: string }) {
  const stop = (e: { stopPropagation: () => void }) => e.stopPropagation()
  return (
    <TooltipProvider delay={120}>
      <Tooltip>
        <TooltipTrigger
          render={<span />}
          aria-label="Show description"
          onClick={stop}
          onDoubleClick={stop}
          onPointerDown={stop}
          className="flex size-4 shrink-0 items-center justify-center rounded-full text-muted-foreground/60 transition-colors hover:bg-[var(--signal)]/15 hover:text-[var(--signal)]"
        >
          <Info className="size-3.5" />
        </TooltipTrigger>
        <TooltipContent
          side="top"
          className="max-w-[340px] whitespace-normal text-left text-[12px] leading-relaxed"
        >
          {description}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}
