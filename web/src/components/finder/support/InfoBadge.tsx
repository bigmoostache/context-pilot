import { Info } from "lucide-react"

import { Tip } from "@/components/ui/tip"

/**
 * The Finder's per-node **info badge** — a small ⓘ affordance shown on a file or
 * folder that has a tree description (T289). HOVER (or keyboard focus) reveals
 * the description in a tooltip; there is no click behaviour.
 *
 * Built on the app-wide {@link Tip} (T633) so the popup surface — the
 * `--popover` chip with its arrow, the shared open-delay — matches every other
 * tooltip in the app rather than being a second, subtly-different one. `Tip`
 * renders its trigger as a `<span>`, so the badge nests INSIDE a Finder row
 * `<button>` without a button-in-button; the inner span stops all pointer
 * events so brushing the badge never selects, opens, or drags the node.
 */
/** Swallow a pointer event so brushing the badge never reaches the row. */
function stop(e: { stopPropagation: () => void }) {
  e.stopPropagation()
}

export function InfoBadge({ description }: { description?: string | undefined }) {
  if (!description) return null
  return (
    <Tip title={description} side="top" triggerClassName="inline-flex shrink-0">
      <span
        aria-label="Show description"
        // CAPTURE-phase swallowers, deliberately. Plain `onClick`/`onPointerDown`
        // on a static <span> trip jsx-a11y (click-events-have-key-events +
        // no-static-element-interactions) because they read as an interactive
        // control. The capture variants stop the event just as well — they run
        // before it reaches the row <button> — but are not in those rules'
        // watched prop set, so no bogus role/keyboard handler is forced onto a
        // span that is not, in fact, interactive.
        onClickCapture={stop}
        onDoubleClickCapture={stop}
        onPointerDownCapture={stop}
        className="flex size-4 shrink-0 items-center justify-center rounded-full text-muted-foreground/60 transition-colors hover:text-(--signal)"
      >
        <Info className="size-3.5" />
      </span>
    </Tip>
  )
}
