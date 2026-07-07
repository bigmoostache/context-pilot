// ── Main-thread stall (event-loop lag) monitor ───────────────────────
//
// The UNIVERSAL freeze detector — the one signal that cannot miss a multi-second
// hang. LoAF, Long Tasks and INP are all Chromium-only and (LoAF) accounted
// per-frame, so they go dark on Firefox/Safari and can under-report a freeze
// that is a *storm of cheap tasks* rather than one long task. This monitor has
// none of those blind spots because it measures the event loop itself.
//
// Mechanism: a `requestAnimationFrame` heartbeat. The browser only fires rAF
// when the main thread is free to produce a frame, so ANY main-thread block —
// a long synchronous task, a saturating burst of many small tasks, a forced
// reflow, a big GC pause — DELAYS the next callback by exactly the block's
// duration. Measuring the gap between successive callbacks yields the true
// stall time. A 2s freeze surfaces as a single ~2000ms gap, browser-agnostic.
//
// Backgrounded tabs legitimately throttle rAF to ~1fps, which would look like a
// perpetual stall, so readings taken while `document.hidden` are dropped and the
// clock is reset on every return to visibility (the post-background catch-up
// frame is not a stall).

import { record } from "./store"

/** Gap above which a frame delay counts as a stall. ~9 dropped 16.7ms frames —
 *  comfortably above render-cadence noise, low enough to catch a 150ms hitch. */
const STALL_THRESHOLD_MS = 150

/**
 * Start the rAF event-loop-lag heartbeat. Records a `stall` event whenever the
 * gap between animation frames exceeds {@link STALL_THRESHOLD_MS} while the page
 * is visible. Returns a disposer that stops the loop.
 */
export function initStallMonitor(): () => void {
  let last = performance.now()
  let rafId = 0
  let stopped = false

  // Returning from a backgrounded tab OR regaining window focus produces one
  // huge (throttled) gap that is NOT a main-thread stall — reset the clock so
  // it isn't recorded. Firefox throttles rAF to ~1fps whenever the WINDOW is
  // unfocused, even while the tab stays "visible" (document.hidden === false),
  // so focus transitions matter as much as visibility ones.
  const onVisibility = () => {
    if (!document.hidden) last = performance.now()
  }
  const onFocus = () => {
    last = performance.now()
  }
  document.addEventListener("visibilitychange", onVisibility)
  window.addEventListener("focus", onFocus)
  window.addEventListener("blur", onFocus)

  const tick = () => {
    if (stopped) return
    const now = performance.now()
    const gap = now - last
    last = now
    // Only record while the page is visible AND the window is focused — an
    // unfocused window's rAF is throttled, so a "gap" then is an artifact, not
    // a freeze. The Web-Worker heartbeat (./heartbeat) is the throttle-immune
    // signal that still catches a REAL block while unfocused.
    if (gap > STALL_THRESHOLD_MS && !document.hidden && document.hasFocus()) {
      record({ kind: "stall", gap: Math.round(gap), ts: Date.now() })
    }
    rafId = requestAnimationFrame(tick)
  }
  rafId = requestAnimationFrame(tick)

  return () => {
    stopped = true
    cancelAnimationFrame(rafId)
    document.removeEventListener("visibilitychange", onVisibility)
    window.removeEventListener("focus", onFocus)
    window.removeEventListener("blur", onFocus)
  }
}
