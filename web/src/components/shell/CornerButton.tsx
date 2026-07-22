import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

/**
 * A fixed screen-corner action button — the mobile chrome primitive for the
 * top-left / top-right "always-reachable" controls.
 *
 * On a phone (and especially a standalone home-screen web app) the very top of
 * the screen is the one region guaranteed reachable and unobscured by page
 * scroll — but it's also where the OS draws the translucent status bar. A naive
 * `fixed top-0` control lands *behind* the clock/battery and can't be tapped
 * (T621). This component encodes the correct recipe once so every corner control
 * shares it:
 *
 *   • `fixed` to the corner so it never scrolls away;
 *   • `top` offset by `env(safe-area-inset-top)` so it clears the status bar in
 *     standalone (and resolves to 0 in a plain browser tab — unchanged there);
 *   • a `backdrop-blur` pad behind the button so it stays legible over whatever
 *     content scrolls beneath it;
 *   • a card-style tap target ≥36px.
 *
 * `side` picks the corner (`"left"` / `"right"`). The caller supplies the glyph
 * as children and the accessible `label`. `className` is merged last so a
 * consumer can override the z-index for its own stacking context (e.g. a control
 * that must sit *under* a scrim so a tap while an overlay is open dismisses it).
 *
 * NOTE ON PLACEMENT: this lives in the desktop `components/` tree because that
 * is the mirror's source-of-truth side (design §5 — every mobile file has a
 * path-parity desktop twin). It is mobile chrome and is consumed only through
 * its `mobile-components/shell/CornerButton` stub; desktop surfaces keep their
 * full `TopBar` and don't render it.
 */
export function CornerButton({
  side,
  label,
  onClick,
  children,
  className,
}: {
  side: "left" | "right"
  label: string
  onClick: () => void
  children: ReactNode
  /** merged last — override the z-index / positioning for a specific stack */
  className?: string
}) {
  return (
    <div
      className={cn(
        "fixed top-[calc(env(safe-area-inset-top)+0.375rem)] z-30 rounded-2xl p-1 backdrop-blur-md",
        side === "left" ? "left-1.5" : "right-1.5",
        className,
      )}
    >
      <button
        onClick={onClick}
        aria-label={label}
        className="card-shadow flex size-9 items-center justify-center rounded-lg border border-border bg-card/95 text-foreground/80 transition-colors hover:bg-muted active:bg-muted"
      >
        {children}
      </button>
    </div>
  )
}
