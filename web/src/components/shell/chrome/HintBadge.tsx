import { useEffect, useRef } from "react"
import { animate } from "animejs"
import { prefersReducedMotion } from "@/lib/utils"

/**
 * The shortcut letter that appears at a control's bottom-right corner while
 * ⌘/Ctrl is held.
 *
 * QUIET ON PURPOSE, in colour and motion: it is metadata about a control, not
 * a control — the reasoning that puts thread rows' idle actions on
 * `muted-foreground`. A `--signal` pill on a spring was tried: loudest thing in
 * the rail, and a spring overshoots by definition — a boing at 13px.
 *
 * ITS OWN MODULE because two unrelated surfaces show one: the rail's thread
 * actions (New thread · Search) and the theme toggle's two segments. Exporting
 * it from `TopBar` would close an import cycle — `TopBar` already imports
 * `ThemeToggle` — which `import-x/no-cycle` rejects outright.
 *
 * THREE THINGS THIS COMPONENT HAS TO GET RIGHT:
 *
 * 1. NO ENTRANCE ON MOUNT. Its hosts re-render for reasons unrelated to the
 *    modifier (agent switch, view change, a theme flip), and a badge animating
 *    in on each of those would be noise. `firstRef` paints once; only CHANGES
 *    animate.
 *
 * 2. THE ANIMATION OWNS `opacity` AND `transform`, so neither may also be set
 *    by a Tailwind class — a `transition-opacity` utility and an anime.js tween
 *    writing the same inline property fight, and the winner depends on frame
 *    timing. Both live in JS here; the classes carry only static styling.
 *
 * 3. `pointer-events-none` — the badge sits over its host's own hit area, and
 *    without it would swallow the click it exists to advertise.
 *
 * The host must be `relative`; the badge positions against it.
 */
export function HintBadge({ label, shown }: { label: string; shown: boolean }) {
  const ref = useRef<HTMLSpanElement>(null)
  const firstRef = useRef(true)

  useEffect(() => {
    const el = ref.current
    if (!el) return

    // See note 1 — mount paints, it does not animate.
    if (firstRef.current) {
      firstRef.current = false
      el.style.opacity = shown ? "1" : "0"
      el.style.transform = shown ? "scale(1)" : "scale(0.9)"
      return
    }

    if (prefersReducedMotion()) {
      el.style.opacity = shown ? "1" : "0"
      el.style.transform = "scale(1)"
      return
    }

    animate(
      el,
      shown
        ? {
            opacity: [0, 1],
            scale: [0.9, 1],
            // Decelerating, and NEVER overshooting: it arrives and stops. This
            // was a spring, which by definition crosses its target and settles
            // back — read as a boing on something this small.
            duration: 150,
            ease: "outQuad",
          }
        : {
            opacity: 0,
            scale: 0.9,
            // Shorter than the entrance: releasing the modifier is a
            // dismissal, and a slow goodbye draws the eye back to something
            // the user has finished with.
            duration: 110,
            ease: "outQuad",
          },
    )
  }, [shown])

  return (
    <span
      ref={ref}
      aria-hidden
      className="pointer-events-none absolute right-0 bottom-0 rounded-full bg-muted px-[4px] text-[9px] leading-[13px] font-semibold text-foreground/70"
    >
      {label}
    </span>
  )
}
