import { useEffect, useLayoutEffect, useRef, type RefObject } from "react"
import { prefersReducedMotion } from "@/lib/utils"

/** Ease-out with a long tail: quick start, soft landing, no overshoot. */
const GLIDE =
  "transform 320ms cubic-bezier(0.32, 0.72, 0, 1), height 320ms cubic-bezier(0.32, 0.72, 0, 1)"

/**
 * Selected-row highlight that glides between rows instead of jumping.
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
  const lastRef = useRef<{ id: string; key: string } | null>(null)

  useLayoutEffect(() => {
    const root = containerRef.current
    const el = ref.current
    if (!root || !el) return
    const place = (animate: boolean) => {
      const row = root.querySelector<HTMLElement>(`[data-row-id="${CSS.escape(selectedId)}"]`)
      if (!row) {
        el.style.opacity = "0"
        return
      }
      el.style.transition = animate ? GLIDE : "none"
      el.style.transform = `translateY(${row.offsetTop}px)`
      el.style.height = `${row.offsetHeight}px`
      el.style.opacity = "1"
    }
    const prev = lastRef.current
    const glide =
      prev !== null &&
      prev.key === layoutKey &&
      prev.id !== selectedId &&
      el.style.opacity === "1" &&
      !prefersReducedMotion()
    place(glide)
    lastRef.current = { id: selectedId, key: layoutKey }

    // Rows added, removed or resized: follow without animating. The observer
    // also fires once on `observe`; skip it or it would cancel the glide.
    let primed = false
    const ro = new ResizeObserver(() => {
      if (primed) place(false)
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
      className="card-shadow pointer-events-none absolute inset-x-2 top-0 rounded-lg bg-card opacity-0"
    />
  )
}
