// ── Web-Worker heartbeat — the DEFINITIVE main-thread freeze detector ──
//
// The rAF stall watchdog (./stall) has one blind spot it cannot escape:
// Firefox (and Chromium) THROTTLE `requestAnimationFrame` to ~1 fps whenever
// the tab's window loses focus — even while the tab itself is "visible"
// (`document.hidden` stays false, since hidden only tracks tab visibility
// WITHIN its window, not window focus). So a user working in another window
// makes the cockpit's rAF fire once a second, and the stall watchdog reports a
// multi-second "gap" that is NOT a freeze at all — a measurement artifact that
// pollutes every capture and survives any fix.
//
// A Web Worker runs on its OWN thread, which the browser NEVER throttles for
// focus/visibility. So a worker that posts a heartbeat on a fixed interval is
// an unthrottled clock. The MAIN thread records the gap between the heartbeats
// it PROCESSES: if the main thread is genuinely blocked (a long task, a burst
// of cheap tasks, a forced reflow, a GC pause), it can't run the worker's
// `onmessage` handler, so the measured gap balloons by exactly the block's
// duration. If instead the big rAF "stall" was mere focus-throttling, the
// worker's messages were still processed on time and this detector stays quiet
// — which is precisely how we tell a REAL freeze from a throttling artifact.
//
// The worker is created from an inline Blob URL so it ships in this one file
// with no separate asset and no bundler worker plumbing.

import { record } from "./store"

/** Worker heartbeat cadence. Short enough to localise a freeze, long enough to
 *  be negligible overhead. */
const BEAT_INTERVAL_MS = 250

/** A processed-heartbeat gap above (interval + this slack) counts as a real
 *  main-thread block. Slack absorbs normal scheduling jitter. */
const BLOCK_SLACK_MS = 200

/** Worker body: post an incrementing tick on a fixed interval, forever. Kept as
 *  a string so it can be wrapped in a Blob — it runs in the worker's OWN global
 *  scope (no DOM, no imports), hence the bare `setInterval`/`postMessage`. */
const WORKER_SRC = `let n=0;setInterval(()=>{postMessage(++n)},${BEAT_INTERVAL_MS});`

/** Disposer used when the heartbeat can't start (no Worker/Blob support) — a
 *  non-empty body so it isn't flagged as an empty function; there is genuinely
 *  nothing to tear down. */
const noHeartbeat = (): void => {
  /* nothing was started */
}

/**
 * Start the Web-Worker heartbeat. Records a `block` event whenever the main
 * thread fails to process a heartbeat on time (the true, throttle-immune freeze
 * signal). Returns a disposer that terminates the worker and revokes its URL.
 *
 * Degrades gracefully: if Workers or Blob URLs are unavailable, it no-ops (the
 * rAF stall watchdog remains as the fallback signal).
 */
export function initHeartbeat(): () => void {
  if (typeof Worker === "undefined" || typeof URL.createObjectURL !== "function") {
    return noHeartbeat
  }

  let url: string
  let worker: Worker
  try {
    url = URL.createObjectURL(new Blob([WORKER_SRC], { type: "text/javascript" }))
    worker = new Worker(url)
  } catch {
    return noHeartbeat
  }

  let last = performance.now()
  // A backgrounded (document.hidden) tab throttles even worker timers in some
  // browsers, which would masquerade as a main-thread block. Reset the clock on
  // return to visibility and only record while visible — so a `block` is always
  // a GENUINE main-thread freeze, never a background-throttle artifact. (Focus
  // throttling does NOT reach workers, so no hasFocus guard is needed here —
  // that immunity is the whole point of the worker signal.)
  const onVisibility = () => {
    if (!document.hidden) last = performance.now()
  }
  document.addEventListener("visibilitychange", onVisibility)

  worker.addEventListener("message", () => {
    const now = performance.now()
    const gap = now - last
    last = now
    // The worker ticked every BEAT_INTERVAL_MS on its unthrottled thread; the
    // excess over (interval + slack) is main-thread time we FAILED to service
    // the message — i.e. the main thread was blocked for that long.
    const blocked = gap - BEAT_INTERVAL_MS
    if (blocked > BLOCK_SLACK_MS && !document.hidden) {
      record({ kind: "block", blocked: Math.round(blocked), ts: Date.now() })
    }
  })

  return () => {
    document.removeEventListener("visibilitychange", onVisibility)
    worker.terminate()
    URL.revokeObjectURL(url)
  }
}
