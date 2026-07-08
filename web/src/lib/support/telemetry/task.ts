// ── Named block-timing (the stall ATTRIBUTION layer) ─────────────────
//
// The rAF stall watchdog (./stall) is universal but ANONYMOUS — it reports that
// the main thread was blocked for N ms, not *what* blocked it. On Chromium the
// Long Animation Frames API names the culprit script for free; Firefox/Safari
// have no such API, so a multi-second freeze there is a nameless gap.
//
// {@link measure} closes that gap: wrap a suspected synchronous hot path (an
// SSE-delta fold, a poll reconcile, a big JSON.parse) and it times the call and
// records a labelled {@link TaskEvent} when the block crosses the threshold. If
// the freeze lives in an instrumented path, the HUD then shows a named entry
// (e.g. `sse:apply 7900ms`) right beside the anonymous stall — turning "the
// main thread blocked for 8 s" into "*this* path blocked for 8 s", on every
// browser.

import { record } from "./store"

/**
 * Time a synchronous block under `label`, recording a {@link TaskEvent} for
 * EVERY call. Transparent: returns `fn`'s value and re-throws its error (the
 * timing runs in `finally`, so a throwing path is still attributed). Overhead
 * is one `performance.now()` pair — negligible.
 *
 * There is deliberately NO minimum-duration gate here: a freeze can be a storm
 * of individually-cheap ops (hundreds of sub-10ms SSE applies), so every span
 * must reach the store, which aggregates count/total/max per label (the
 * burst-catcher) and only ADDS big-enough ones to the worst-list/ring.
 */
export function measure<T>(label: string, fn: () => T): T {
  const t0 = performance.now()
  try {
    return fn()
  } finally {
    const duration = performance.now() - t0
    record({ kind: "task", label, duration: Math.round(duration * 100) / 100, ts: Date.now() })
  }
}

/**
 * Async twin of {@link measure} — brackets an awaited span end-to-end and
 * records a {@link TaskEvent} for every call.
 *
 * A span includes any network wait, so on a REMOTE endpoint this over-reports
 * (the wait isn't a main-thread block). But the cockpit talks to a same-origin
 * orchestrator on `127.0.0.1` where round-trips are sub-millisecond, so a
 * multi-second `load:*` span is overwhelmingly the SYNCHRONOUS `res.json()`
 * deserialize + reshape of the payload — i.e. real main-thread cost. Correlate
 * a `load:*` entry with a same-timestamp main-thread block to name which polled
 * endpoint's parse burned the freeze.
 */
export async function measureAsync<T>(label: string, fn: () => Promise<T>): Promise<T> {
  const t0 = performance.now()
  try {
    return await fn()
  } finally {
    const duration = performance.now() - t0
    record({ kind: "task", label, duration: Math.round(duration * 100) / 100, ts: Date.now() })
  }
}
