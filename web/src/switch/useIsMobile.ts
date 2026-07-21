import { useState } from "react"

/** Viewport width at/under which the mobile component tree is chosen. */
const MOBILE_QUERY = "(max-width: 768px)"

/**
 * Resolve whether the mobile component tree should render — **once, at first
 * paint**, and frozen for the session.
 *
 * The `useState` initializer runs a single time, so a live viewport resize
 * across the breakpoint does NOT flip the value: that would remount the entire
 * tree and destroy component-local state (scroll, open dialogs, form input).
 * Crossover is instead surfaced as a "reload for the {mobile|desktop} layout"
 * prompt elsewhere (design §6), leaving the actual swap user-initiated.
 *
 * SSR-safe: with no `window` (a future prerender/hydration step) it resolves to
 * `false` (desktop) rather than throwing on `window.matchMedia`.
 */
export function useIsMobile(): boolean {
  const [isMobile] = useState(
    () => typeof window !== "undefined" && window.matchMedia(MOBILE_QUERY).matches,
  )
  return isMobile
}
