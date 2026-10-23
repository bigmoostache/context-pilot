import { useLayoutEffect, useState } from "react"
import type { RefObject } from "react"

/** Viewport box for a popup opened under an anchor. */
export interface BelowAnchor {
  top: number
  left: number
  minWidth: number
}

/**
 * Viewport position just under `anchorRef` while `open`, for `position: fixed`
 * popups (menus, combo lists). The ribbon card scrolls horizontally (overflow
 * auto), which would clip an absolutely positioned popup to the card's height.
 * Null while closed or before the anchor is measured.
 */
export function useBelowAnchor(
  anchorRef: RefObject<HTMLElement | null>,
  open: boolean,
): BelowAnchor | null {
  const [at, setAt] = useState<BelowAnchor | null>(null)
  useLayoutEffect(() => {
    const r = open ? anchorRef.current?.getBoundingClientRect() : undefined
    setAt(r ? { top: r.bottom + 2, left: r.left, minWidth: r.width } : null)
  }, [anchorRef, open])
  return at
}
