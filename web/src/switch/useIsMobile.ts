import { useState } from "react"

/**
 * The media query that selects the mobile component tree — an OR of two signals
 * (comma in a media query is a union):
 *
 *   • `(max-width: 768px)` — the CLASSIC responsive breakpoint (Bootstrap `md`,
 *     Tailwind `md`). A narrow window — phone OR a dragged-narrow desktop — gets
 *     the mobile tree, which is the conventional responsive intent.
 *   • `(pointer: coarse)` — a touch-primary device (phone / tablet / foldable)
 *     whose viewport happens to exceed 768px still gets the touch-optimised
 *     tree. This is the robustness signal: width alone misclassifies a portrait
 *     tablet, `pointer` catches it.
 *
 * Together they are the industry-standard "is this a small-or-touch device"
 * probe — robust across form factors without over-reaching.
 */
const MOBILE_QUERY = "(max-width: 768px), (pointer: coarse)"

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
