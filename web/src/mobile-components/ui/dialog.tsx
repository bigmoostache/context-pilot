import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"

import { cn } from "@/lib/utils"

/**
 * shadcn-style Dialog (mobile twin) built on Base UI's Dialog primitive.
 *
 * Same Portal / focus-trap / scroll-lock / Esc-to-close contract as the desktop
 * twin — the divergence is purely presentational: where desktop centres a
 * spring-in card in the viewport, the phone anchors a **bottom sheet** (full
 * width, rounded top, thumb-reachable, safe-area padded) that slides up from the
 * bottom edge. A consumer that wants a full-screen sheet instead (e.g. the users
 * dialog) still overrides via `className` — tailwind-merge keeps the caller's
 * classes last, so the bottom-sheet default never fights an explicit override.
 */

function Dialog(props: DialogPrimitive.Root.Props) {
  return <DialogPrimitive.Root {...props} />
}

function DialogTrigger(props: DialogPrimitive.Trigger.Props) {
  return <DialogPrimitive.Trigger {...props} />
}

function DialogClose(props: DialogPrimitive.Close.Props) {
  return <DialogPrimitive.Close {...props} />
}

function DialogBackdrop({ className, ...props }: DialogPrimitive.Backdrop.Props) {
  return (
    <DialogPrimitive.Backdrop
      data-slot="dialog-backdrop"
      className={cn(
        "fixed inset-0 z-50 bg-black/40 backdrop-blur-[3px] transition-opacity duration-200",
        "data-ending-style:opacity-0 data-starting-style:opacity-0",
        className,
      )}
      {...props}
    />
  )
}

/**
 * The dialog surface — a bottom-anchored sheet on mobile. Spans the full width,
 * rounds only its top corners, caps its height at 90vh with an internal scroll,
 * and pads its bottom for the home-indicator safe area. Slides up on open and
 * down on close via Base UI's `data-[starting-style]` / `data-[ending-style]`
 * translate transition (desktop uses a centred scale spring instead).
 */
function DialogContent({ className, children, ...props }: DialogPrimitive.Popup.Props) {
  return (
    <DialogPrimitive.Portal>
      <DialogBackdrop />
      <DialogPrimitive.Popup
        data-slot="dialog-content"
        className={cn(
          "fixed inset-x-0 bottom-0 z-50 max-h-[90vh] overflow-y-auto",
          "rounded-t-2xl border-t border-border bg-popover text-popover-foreground",
          "pb-[env(safe-area-inset-bottom)] shadow-(--shadow-pop) outline-none",
          "transition-transform duration-200 ease-out",
          "data-ending-style:translate-y-full data-starting-style:translate-y-full",
          className,
        )}
        {...props}
      >
        {children}
      </DialogPrimitive.Popup>
    </DialogPrimitive.Portal>
  )
}

function DialogTitle({ className, ...props }: DialogPrimitive.Title.Props) {
  return (
    <DialogPrimitive.Title
      data-slot="dialog-title"
      className={cn("text-[15px] font-semibold tracking-tight text-foreground", className)}
      {...props}
    />
  )
}

function DialogDescription({ className, ...props }: DialogPrimitive.Description.Props) {
  return (
    <DialogPrimitive.Description
      data-slot="dialog-description"
      className={cn("text-[12px] text-muted-foreground", className)}
      {...props}
    />
  )
}

export {
  Dialog,
  DialogTrigger,
  DialogClose,
  DialogContent,
  DialogBackdrop,
  DialogTitle,
  DialogDescription,
}
