import { Popover as PopoverPrimitive } from "@base-ui/react/popover"

import { cn } from "@/lib/utils"

/**
 * shadcn-style Popover built on Base UI's Popover primitive.
 *
 * Like the project's Dialog, the popup is **portaled** into `document.body`, so
 * it escapes any ancestor that clips overflow or establishes a containing block
 * (e.g. a scrolling Finder view) and always renders above the surface. Base UI
 * also gives focus management, outside-click + Esc dismissal, and `aria` wiring
 * for free. The Root accepts `openOnHover` (+ `delay`) so a trigger can open on
 * hover as well as click — the pattern the Finder's info badge uses.
 */
function Popover(props: PopoverPrimitive.Root.Props) {
  return <PopoverPrimitive.Root {...props} />
}

function PopoverTrigger(props: PopoverPrimitive.Trigger.Props) {
  return <PopoverPrimitive.Trigger data-slot="popover-trigger" {...props} />
}

/**
 * The popover surface. Positioned by Base UI relative to the trigger (default
 * above-centered). Children own their padding/scroll. Motion reuses the
 * tw-animate vocabulary keyed on Base UI's `data-open` / `data-closed` state so
 * both open and close animate.
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
            "z-50 origin-(--transform-origin) rounded-lg border border-border bg-popover p-3 text-popover-foreground pop-shadow outline-none",
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
