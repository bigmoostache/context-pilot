import type { ReactNode } from "react"
import { Tooltip as TooltipPrimitive } from "@base-ui/react/tooltip"

import { cn } from "@/lib/utils"

/**
 * `Tip` — the app-wide tooltip wrapper (T25).
 *
 * Wraps any single trigger element and shows a small, portaled popup on
 * hover/focus that *names* a control and optionally *explains* it — ideal for
 * the app's jargon buttons (Threads / Cockpit / Finder / Conversation …) and
 * the many icon-only controls whose meaning isn't obvious.
 *
 * Composed straight from the Base-UI tooltip primitive (rather than the generic
 * `TooltipContent`) so we own the popup surface *and* its arrow: both read from
 * the theme's `--popover` token, so the chip looks native in light and dark and
 * never clashes. Portaled to `document.body`, escaping the TopBar's `.vibrancy`
 * backdrop-filter containing block (the same fix dialogs use). Focus + keyboard
 * a11y come for free.
 *
 * The shared {@link TooltipProvider} (mounted once at the app root) governs the
 * open delay so every tooltip across the app feels consistent and intentional.
 */
export function Tip({
  title,
  body,
  side = "bottom",
  sideOffset = 7,
  triggerClassName,
  children,
}: {
  title: ReactNode
  body?: ReactNode | undefined
  side?: "top" | "bottom" | "left" | "right" | undefined
  sideOffset?: number | undefined
  /** Class for the trigger wrapper span — e.g. `block` to preserve full-width children. */
  triggerClassName?: string | undefined
  children: ReactNode
}) {
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger
        render={(props) => {
          // `tabIndex: 0` keeps the tooltip reachable by keyboard even when the
          // wrapped content is non-interactive (e.g. a plain label/icon). Seeded
          // first so an explicit tabIndex on `props` still wins.
          const merged = { tabIndex: 0, ...props, className: cn(props.className, triggerClassName) }
          return <span {...merged}>{children}</span>
        }}
      />
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Positioner side={side} sideOffset={sideOffset} className="z-50">
          <TooltipPrimitive.Popup
            className={cn(
              "flex max-w-[232px] flex-col gap-0.5 rounded-lg border border-border bg-popover px-3 py-2 text-left",
              "shadow-[var(--shadow-pop)] outline-none",
              "origin-(--transform-origin) transition-[transform,opacity] duration-150",
              "data-[starting-style]:scale-95 data-[starting-style]:opacity-0",
              "data-[ending-style]:scale-95 data-[ending-style]:opacity-0",
            )}
          >
            <TooltipPrimitive.Arrow className="data-[side=bottom]:-top-[5px] data-[side=top]:-bottom-[5px] data-[side=left]:-right-[5px] data-[side=right]:-left-[5px]">
              <span className="block size-2.5 rotate-45 rounded-[2px] border-b border-r border-border bg-popover" />
            </TooltipPrimitive.Arrow>
            <span className="text-[12px] font-semibold leading-tight text-foreground">{title}</span>
            {body ? (
              <span className="text-[11px] leading-snug text-muted-foreground">{body}</span>
            ) : null}
          </TooltipPrimitive.Popup>
        </TooltipPrimitive.Positioner>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  )
}
