import { useEffect, useLayoutEffect, useRef, type RefObject } from "react"
import { prefersReducedMotion } from "@/lib/utils"

/** Soft landing with a long tail, no overshoot. */
const EASE = "cubic-bezier(0.32, 0.72, 0, 1)"

/** Where the highlight sits: top offset and height in px. */
interface Box {
  y: number
  h: number
}

const frame = (b: Box, squeeze = 1): Keyframe => ({
  transform: `translateY(${b.y}px) scaleX(${squeeze})`,
  height: `${b.h}px`,
})

/**
 * Elastic move: the leading edge races ahead while the trailing edge lags, so
 * the card stretches toward the new row and then snaps shut onto it, narrowing
 * slightly mid-flight. Then a signal-tinted glow blooms on landing.
 */
function glide(card: HTMLElement, glow: HTMLElement, from: Box, to: Box): void {
  const down = to.y > from.y
  const lead = down ? 0.9 : 0.3
  const trail = down ? 0.3 : 0.9
  const top = from.y + (to.y - from.y) * trail
  const bottom = from.y + from.h + (to.y + to.h - from.y - from.h) * lead
  for (const a of [...card.getAnimations(), ...glow.getAnimations()]) a.cancel()
  card.animate(
    [frame(from), { ...frame({ y: top, h: bottom - top }, 0.97), offset: 0.45 }, frame(to)],
    { duration: 460, easing: EASE },
  )
  glow.animate(
    [
      { opacity: 0, transform: "scale(0.98)" },
      { opacity: 1, transform: "scale(1)", offset: 0.3 },
      { opacity: 0, transform: "scale(1.035)" },
    ],
    { duration: 800, delay: 260, easing: "ease-out", fill: "backwards" },
  )
}

/**
 * Selected-row highlight that glides between rows instead of jumping, plus the
 * cursor spotlight the `.agent-row` hover style reads (`--mx`/`--my`).
 *
 * Render it as the first child of a `relative` list whose rows carry
 * `data-row-id`. It follows the row whose id is `selectedId`. It only animates
 * when the selection changes inside the same list; it snaps on mount, when
 * `layoutKey` changes (another list is shown), when the list resizes, and when
 * the user prefers reduced motion. Styles are written straight to the DOM, so a
 * move costs no React render.
 */
export function SelectionGlide({
  containerRef,
  selectedId,
  layoutKey,
}: {
  containerRef: RefObject<HTMLElement | null>
  selectedId: string
  layoutKey: string
}) {
  const ref = useRef<HTMLDivElement>(null)
  const glowRef = useRef<HTMLDivElement>(null)
  const lastRef = useRef<{ id: string; key: string; box: Box } | null>(null)

  useLayoutEffect(() => {
    const root = containerRef.current
    const el = ref.current
    const glow = glowRef.current
    if (!root || !el || !glow) return
    const measure = (): Box | null => {
      const row = root.querySelector<HTMLElement>(`[data-row-id="${CSS.escape(selectedId)}"]`)
      return row ? { y: row.offsetTop, h: row.offsetHeight } : null
    }
    const place = (box: Box | null) => {
      if (!box) {
        el.style.opacity = "0"
        return
      }
      el.style.transform = `translateY(${box.y}px)`
      el.style.height = `${box.h}px`
      el.style.opacity = "1"
      if (lastRef.current) lastRef.current.box = box
    }
    const box = measure()
    const prev = lastRef.current
    place(box)
    if (
      box &&
      prev !== null &&
      prev.key === layoutKey &&
      prev.id !== selectedId &&
      !prefersReducedMotion()
    ) {
      glide(el, glow, prev.box, box)
    }
    if (box) lastRef.current = { id: selectedId, key: layoutKey, box }

    // Rows added, removed or resized: follow without animating. The observer
    // also fires once on `observe`; skip it or it would cancel the glide.
    let primed = false
    const ro = new ResizeObserver(() => {
      if (primed) place(measure())
      primed = true
    })
    ro.observe(root)
    return () => ro.disconnect()
  }, [containerRef, selectedId, layoutKey])

  // Cursor spotlight: one delegated listener feeds the hovered row its local
  // pointer position, which `.agent-row::before` centres its glow on.
  useEffect(() => {
    const root = containerRef.current
    if (!root) return
    const onMove = (e: PointerEvent) => {
      if (!(e.target instanceof Element)) return
      const row = e.target.closest<HTMLElement>("[data-row-id]")
      if (!row) return
      const r = row.getBoundingClientRect()
      row.style.setProperty("--mx", `${e.clientX - r.left}px`)
      row.style.setProperty("--my", `${e.clientY - r.top}px`)
    }
    root.addEventListener("pointermove", onMove, { passive: true })
    return () => root.removeEventListener("pointermove", onMove)
  }, [containerRef])

  return (
    <div
      ref={ref}
      aria-hidden
      className="card-shadow pointer-events-none absolute inset-x-2 top-0 origin-center rounded-lg bg-card opacity-0"
    >
      <div
        ref={glowRef}
        className="absolute inset-0 rounded-[inherit] opacity-0 shadow-[0_0_0_1.5px_color-mix(in_oklab,var(--signal)_45%,transparent),0_0_18px_color-mix(in_oklab,var(--signal)_30%,transparent)]"
      />
    </div>
  )
}
