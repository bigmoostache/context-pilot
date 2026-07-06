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
  /** Snapshot of the current selection — read at drag start for additive (⌘/Ctrl) unions. */
  getSelected: () => Set<string>
  /** Replace the selection with the absolute set computed for the current band. */
  onChange: (next: Set<string>) => void
  /** Empty-space click (press→release with no drag) — clears the selection. */
  onEmptyClick: () => void
}

/** Pixels the pointer must travel before a press is promoted to a marquee drag. */
const DRAG_THRESHOLD = 4

/**
 * Finder rubber-band (box) selection.
 *
 * Spread `handlers` onto the grid/list scroll surface (which must be
 * `position: relative`) and render `band` as an absolutely-positioned child.
 * A drag that begins on empty space — not on a `[data-finder-item]` cell —
 * sweeps a marquee; every cell whose box intersects it is selected live.
 * Holding ⌘/Ctrl unions the swept cells onto the selection that existed when
 * the drag began; otherwise the marquee replaces the selection. A press with
 * no drag clears the selection.
 *
 * Uses pointer handlers (no window listeners) and live `getBoundingClientRect()`
 * hit-testing, so it stays correct as the surface scrolls mid-drag. `didDrag()`
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
  const drag = useRef<{
    ox: number
    oy: number
    additive: boolean
    base: Set<string>
    moved: boolean
  } | null>(null)
  // One-shot: set on a completed sweep, consumed by the host's click handler.
  const justDragged = useRef(false)

  const hitTest = useCallback(
    (l: number, t: number, r: number, b: number): string[] => {
      const root = containerRef.current
      if (!root) return []
      const hits: string[] = []
      root.querySelectorAll<HTMLElement>("[data-finder-item]").forEach((el) => {
        const path = el.getAttribute("data-path")
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
      if ((e.target as HTMLElement).closest("[data-finder-item]")) return // press on a cell → click
      const additive = e.metaKey || e.ctrlKey
      drag.current = {
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
      const d = drag.current
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

      const cr = root.getBoundingClientRect()
      setBand({ left: l - cr.left, top: t - cr.top, width: r - l, height: b - t })
      onChange(new Set([...d.base, ...hitTest(l, t, r, b)]))
    },
    [containerRef, hitTest, onChange],
  )

  const finish = useCallback(() => {
    const d = drag.current
    drag.current = null
    setBand(null)
    if (!d) return
    if (d.moved) justDragged.current = true
    else if (!d.additive) onEmptyClick()
  }, [onEmptyClick])

  /** Consume the one-shot "a sweep just completed" flag; the host skips its click-clear when true. */
  const didDrag = useCallback(() => {
    const was = justDragged.current
    justDragged.current = false
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
