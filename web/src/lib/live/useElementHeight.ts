import { useEffect, useState, type RefObject } from "react"

// ── Live element height (ResizeObserver) ─────────────────────────────
//
// Report the live pixel height of a referenced element, updating as it resizes.
// Factored out of the individual mobile surfaces (design rule M141) so the
// floating-composer / floating-bottom-bar pattern can size its scroll spacer
// from ONE measurement primitive instead of a bespoke `ResizeObserver` inlined
// per surface.
//
// The observer callback is event-driven (not a render-phase setState), so it
// never fires during render; it tracks auto-growing content (e.g. a composer's
// textarea) as it changes height. Returns 0 until the first measurement lands.

/**
 * Track the live height (px) of the element `ref` points at via a
 * `ResizeObserver`. Returns 0 before the first observation. Re-observes if the
 * ref target changes.
 */
export function useElementHeight(ref: RefObject<HTMLElement | null>): number {
  const [height, setHeight] = useState(0)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height
      if (typeof h === "number") setHeight(h)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [ref])
  return height
}
