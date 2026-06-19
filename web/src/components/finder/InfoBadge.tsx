import { Info } from "lucide-react"

import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"

/**
 * The Finder's per-node **info badge** — a small ⓘ affordance shown on a file or
 * folder that has a tree description (T289). Hovering or clicking it opens a
 * popover with the description text.
 *
 * It renders the Popover trigger as a `<span>` (Base UI `render` prop) so it can
 * live INSIDE a Finder row/cell `<button>` without nesting a button in a button.
 * All pointer events are stopped from propagating so interacting with the badge
 * never selects, opens, or drags the underlying node.
 */
export function InfoBadge({ description }: { description: string }) {
  const stop = (e: { stopPropagation: () => void }) => e.stopPropagation()
  return (
    <Popover>
      <PopoverTrigger
        openOnHover
        delay={120}
        render={<span />}
        aria-label="Show description"
        onClick={stop}
        onDoubleClick={stop}
        onPointerDown={stop}
        className="flex size-4 shrink-0 cursor-help items-center justify-center rounded-full text-muted-foreground/60 transition-colors hover:bg-[var(--signal)]/15 hover:text-[var(--signal)]"
      >
        <Info className="size-3.5" />
      </PopoverTrigger>
      <PopoverContent
        side="top"
        className="max-w-[340px] text-[12px] leading-relaxed text-foreground/85"
        onClick={stop}
        onDoubleClick={stop}
      >
        {description}
      </PopoverContent>
    </Popover>
  )
}
