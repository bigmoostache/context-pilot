import { useEffect, useRef } from "react"
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"
import { animate, createSpring } from "animejs"

import { cn, prefersReducedMotion } from "@/lib/utils"

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
  // #6 Sheet rise (anime.js): spring the sheet up + fade on open, for a livelier
  // entrance than the flat CSS translate. The Popup mounts fresh each open (Base
  // UI unmounts it on close), so a mount-effect is the one-shot trigger. anime
  // owns the ENTRY only: transitions are killed while it runs, then the inline
  // transform/opacity/transition it wrote are cleared on completion so Base UI's
  // class-driven `data-ending-style:translate-y-full` governs the CLOSE. The
  // `data-starting-style` entry translate is dropped from the class list so the
  // two never both drive the opening frame. Reduced-motion skips the spring.
  const popupRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = popupRef.current
    if (!el || prefersReducedMotion()) return
    el.style.transition = "none"
    animate(el, {
      translateY: [24, 0],
      opacity: [0, 1],
      ease: createSpring({ stiffness: 320, damping: 30 }),
      onComplete: () => {
        el.style.transform = ""
        el.style.opacity = ""
        el.style.transition = ""
      },
    })
  }, [])

  return (
    <DialogPrimitive.Portal>
      <DialogBackdrop />
      <DialogPrimitive.Popup
        ref={popupRef}
        data-slot="dialog-content"
        className={cn(
          "fixed inset-x-0 bottom-0 z-50 max-h-[90vh] overflow-y-auto",
          "rounded-t-2xl border-t border-border bg-popover text-popover-foreground",
          "pb-[env(safe-area-inset-bottom)] shadow-(--shadow-pop) outline-none",
          "transition-transform duration-200 ease-out",
          "data-ending-style:translate-y-full",
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
