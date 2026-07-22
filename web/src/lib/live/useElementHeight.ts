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
 * Track the live BORDER-BOX height (px) of the element `ref` points at via a
 * `ResizeObserver`. Returns 0 before the first observation. Re-observes if the
 * ref target changes.
 *
 * We read `borderBoxSize` (padding + border included), NOT `contentRect`
 * (content-box, padding EXCLUDED). Consumers size a scroll spacer as a multiple
 * of a floating bar's height so the last row clears the bar — and that bar's
 * height is mostly padding (its `pt-*` + a `pb-[env(safe-area-inset-bottom)]`
 * that's ~34px on a notched phone). A content-box measurement would undershoot
 * by that whole padding band, leaving the last row still tucked under the bar
 * (T637). `observe({box:"border-box"})` populates `borderBoxSize`; the
 * `contentRect` read is a fallback for any engine that leaves it empty.
 */
export function useElementHeight(ref: RefObject<HTMLElement | null>): number {
  const [height, setHeight] = useState(0)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (!entry) return
      const h = entry.borderBoxSize?.[0]?.blockSize ?? entry.contentRect.height
      if (typeof h === "number") setHeight(h)
    })
    ro.observe(el, { box: "border-box" })
    return () => ro.disconnect()
  }, [ref])
  return height
}
