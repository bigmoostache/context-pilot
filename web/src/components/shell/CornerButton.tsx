import { useEffect, useRef, type ReactNode } from "react"
import { animate, createSpring } from "animejs"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { useTopButtons } from "@/lib/providers/topButtons"

/**
 * A fixed screen-corner action button — the mobile chrome primitive for the
 * top-left / top-right "always-reachable" controls (drawer toggle, agents grid,
 * agent settings, archived toggle). The SINGLE place every corner button is
 * rendered, so its look + motion are defined once and every call site inherits
 * them (they pass only `side` / `label` / `onClick` / the glyph).
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
 *   • a circular **glass** target (translucent theme surface + `backdrop-blur`)
 *     so it stays legible over whatever content scrolls beneath it;
 *   • a thumb-sized tap target (56px ≈ 1.5× the old 36px).
 *
 * Motion (anime.js, all `prefers-reduced-motion` guarded — see
 * {@link useCornerMotion}): a spring **entrance** on mount, an icon-swap spring
 * re-fired on every **page transition** (the shared `navKey` from
 * {@link useTopButtons} bumps on each navigation edge), and a press-release
 * **pop** on tap.
 *
 * `side` picks the corner (`"left"` / `"right"`). `className` is merged last so a
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
  const { press, glyphRef } = useCornerMotion()

  return (
    <div
      className={cn(
        "fixed top-[calc(env(safe-area-inset-top)+0.5rem)] z-30",
        side === "left" ? "left-2.5" : "right-2.5",
        className,
      )}
    >
      <button
        onClick={() => {
          press()
          onClick()
        }}
        aria-label={label}
        className={cn(
          // Circular glass target: a translucent theme surface (works in both
          // palettes via color-mix on --surface), a heavy backdrop blur +
          // saturate so whatever scrolls under it reads as frosted glass, a
          // hairline border + inset highlight for the "lifted glass" edge, and a
          // float shadow. 56px ≈ 1.5× the previous 36px.
          "flex size-14 items-center justify-center rounded-full",
          "border border-white/15 text-foreground/85",
          "bg-[color-mix(in_oklab,var(--surface)_55%,transparent)]",
          "shadow-(--shadow-pop) ring-1 ring-white/10 ring-inset",
          "backdrop-blur-xl backdrop-saturate-150",
          "transition-colors active:bg-[color-mix(in_oklab,var(--surface)_72%,transparent)]",
        )}
      >
        {/* the glyph is the animated element — the button itself stays put so
            the backdrop-blur pane doesn't re-rasterise mid-spring */}
        <span ref={glyphRef} className="flex items-center justify-center [&>svg]:size-6">
          {children}
        </span>
      </button>
    </div>
  )
}

/** The corner button's anime.js motion: entrance + page-transition icon swap +
 *  press-release pop. All guarded by `prefers-reduced-motion`. */
function useCornerMotion() {
  const glyphRef = useRef<HTMLSpanElement>(null)
  const { navKey } = useTopButtons()

  // Icon-swap spring: on mount AND on every navigation edge (navKey change) the
  // glyph springs in — scaling up from a shrunk, faded, slightly-rotated start —
  // so a page change animates the corner controls in lock-step (the "transition
  // between buttons" the coordination provider exists for).
  useEffect(() => {
    const el = glyphRef.current
    if (!el || prefersReducedMotion()) return
    animate(el, {
      scale: [0.5, 1],
      opacity: [0, 1],
      rotate: [-35, 0],
      ease: createSpring({ stiffness: 380, damping: 20 }),
    })
  }, [navKey])

  /** Press-release pop — a quick spring back to rest from a shrunk state, fired
   *  on tap for tactile feedback. */
  const press = () => {
    const el = glyphRef.current
    if (!el || prefersReducedMotion()) return
    animate(el, {
      scale: [0.82, 1],
      ease: createSpring({ stiffness: 600, damping: 14 }),
    })
  }

  return { press, glyphRef }
}
