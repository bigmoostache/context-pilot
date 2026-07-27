import { Popover as PopoverPrimitive } from "@base-ui/react/popover"

import { cn } from "@/lib/utils"

/**
 * shadcn-style Popover (mobile twin) built on Base UI's Popover primitive.
 *
 * Portalling, focus management, outside-click + Esc dismissal and `openOnHover`
 * are identical to the desktop twin. A popover stays anchored to its trigger on
 * touch (it's a contextual surface, not a modal), so the only divergence is a
 * width clamp: the popup may not exceed the viewport minus a gutter
 * (`max-w-[calc(100vw-1rem)]`), so a wide popover can never clip off a narrow
 * phone edge.
 */
function Popover(props: PopoverPrimitive.Root.Props) {
  return <PopoverPrimitive.Root {...props} />
}

function PopoverTrigger(props: PopoverPrimitive.Trigger.Props) {
  return <PopoverPrimitive.Trigger data-slot="popover-trigger" {...props} />
}

/**
 * The popover surface. Positioned by Base UI relative to the trigger, clamped to
 * the viewport width so it never overflows a phone edge. Motion reuses the
 * tw-animate vocabulary keyed on Base UI's `data-open` / `data-closed` state.
 */
function PopoverContent({
  className,
  side = "top",
  sideOffset = 6,
  align = "center",
  children,
  ...props
}: PopoverPrimitive.Popup.Props &
  Pick<PopoverPrimitive.Positioner.Props, "align" | "side" | "sideOffset">) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Positioner
        side={side}
        sideOffset={sideOffset}
        align={align}
        className="isolate z-50"
      >
        <PopoverPrimitive.Popup
          data-slot="popover-content"
          className={cn(
            "pop-shadow z-50 max-w-[calc(100vw-1rem)] origin-(--transform-origin) rounded-lg border border-border bg-popover p-3 text-popover-foreground outline-none",
            "data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95",
            "data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95",
            className,
          )}
          {...props}
        >
          {children}
        </PopoverPrimitive.Popup>
      </PopoverPrimitive.Positioner>
    </PopoverPrimitive.Portal>
  )
}

export { Popover, PopoverTrigger, PopoverContent }
