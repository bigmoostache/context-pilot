import { useCallback, useRef, useState } from "react"
import type { MouseEvent as ReactMouseEvent } from "react"

/** Container-relative rectangle for rendering the rubber-band overlay. */
export interface MarqueeBand {
  left: number
  top: number
  width: number
  height: number
}

interface UseMarqueeArgs {
  /** The scrollable view surface that hosts the `[data-finder-item]` cells. */
  containerRef: React.RefObject<HTMLElement | null>
  /** Whether to arm the marquee (grid / list only). */
  enabled: boolean
  /** Snapshot of the current selection — read at dragRef start for additive (⌘/Ctrl) unions. */
  getSelected: () => Set<string>
  /** Replace the selection with the absolute set computed for the current band. */
  onChange: (next: Set<string>) => void
  /** Empty-space click (press→release with no dragRef) — clears the selection. */
  onEmptyClick: () => void
}

/** Pixels the pointer must travel before a press is promoted to a marquee dragRef. */
const DRAG_THRESHOLD = 4

/**
 * Finder rubber-band (box) selection.
 *
 * Spread `handlers` onto the grid/list scroll surface (which must be
 * `position: relative`) and render `band` as an absolutely-positioned child.
 * A dragRef that begins on empty space — not on a `[data-finder-item]` cell —
 * sweeps a marquee; every cell whose box intersects it is selected live.
 * Holding ⌘/Ctrl unions the swept cells onto the selection that existed when
 * the dragRef began; otherwise the marquee replaces the selection. A press with
 * no dragRef clears the selection.
 *
 * Uses pointer handlers (no window listeners) and live `getBoundingClientRect()`
 * hit-testing, so it stays correct as the surface scrolls mid-dragRef. `didDrag()`
 * lets the host suppress the background-click clear that would otherwise fire on
 * release after a sweep (it returns — and resets — a one-shot "just dragged" flag).
 */
export function useMarquee({
  containerRef,
  enabled,
  getSelected,
  onChange,
  onEmptyClick,
}: UseMarqueeArgs) {
  const [band, setBand] = useState<MarqueeBand | null>(null)
  const dragRef = useRef<{
    ox: number
    oy: number
    additive: boolean
    base: Set<string>
    moved: boolean
  } | null>(null)
  // One-shot: set on a completed sweep, consumed by the host's click handler.
  const justDraggedRef = useRef(false)

  const hitTest = useCallback(
    (l: number, t: number, r: number, b: number): string[] => {
      const root = containerRef.current
      if (!root) return []
      const hits: string[] = []
      root.querySelectorAll<HTMLElement>("[data-finder-item]").forEach((el) => {
        const path = el.dataset["path"]
        if (!path) return
        const rc = el.getBoundingClientRect()
        const outside = rc.right < l || rc.left > r || rc.bottom < t || rc.top > b
        if (!outside) hits.push(path)
      })
      return hits
    },
    [containerRef],
  )

  const onPointerDown = useCallback(
    (e: ReactMouseEvent) => {
      if (!enabled || e.button !== 0) return
      if (!(e.target instanceof HTMLElement)) return
      if (e.target.closest("[data-finder-item]")) return // press on a cell → click
      const additive = e.metaKey || e.ctrlKey
      dragRef.current = {
        ox: e.clientX,
        oy: e.clientY,
        additive,
        base: additive ? new Set(getSelected()) : new Set(),
        moved: false,
      }
    },
    [enabled, getSelected],
  )

  const onPointerMove = useCallback(
    (e: ReactMouseEvent) => {
      const d = dragRef.current
      const root = containerRef.current
      if (!d || !root) return
      if (
        !d.moved &&
        Math.abs(e.clientX - d.ox) < DRAG_THRESHOLD &&
        Math.abs(e.clientY - d.oy) < DRAG_THRESHOLD
      ) {
        return
      }
      d.moved = true

      const l = Math.min(d.ox, e.clientX)
      const t = Math.min(d.oy, e.clientY)
      const r = Math.max(d.ox, e.clientX)
      const b = Math.max(d.oy, e.clientY)

      // The band is an absolutely-positioned child of the (scrollable) surface,
      // so its offset is measured from the CONTENT origin, not the viewport.
      // Add the current scroll so a band drawn after scrolling lines up with the
      // pointer instead of floating scrollTop px above it (M14). Hit-testing uses
      // live viewport rects, so it stays correct regardless.
      const cr = root.getBoundingClientRect()
      setBand({
        left: l - cr.left + root.scrollLeft,
        top: t - cr.top + root.scrollTop,
        width: r - l,
        height: b - t,
      })
      onChange(new Set([...d.base, ...hitTest(l, t, r, b)]))
    },
    [containerRef, hitTest, onChange],
  )

  const finish = useCallback(() => {
    const d = dragRef.current
    dragRef.current = null
    setBand(null)
    if (!d) return
    if (d.moved) justDraggedRef.current = true
    else if (!d.additive) onEmptyClick()
  }, [onEmptyClick])

  /** Consume the one-shot "a sweep just completed" flag; the host skips its click-clear when true. */
  const didDrag = useCallback(() => {
    const was = justDraggedRef.current
    justDraggedRef.current = false
    return was
  }, [])

  return {
    band,
    didDrag,
    handlers: enabled
      ? {
          onMouseDown: onPointerDown,
          onMouseMove: onPointerMove,
          onMouseUp: finish,
          onMouseLeave: finish,
        }
      : {},
  }
}
