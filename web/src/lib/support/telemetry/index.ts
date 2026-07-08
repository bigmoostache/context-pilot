// ── Telemetry public surface ─────────────────────────────────────────
//
// Wires the three producers (web-vitals, Long Animation Frames, React
// Profiler) into the shared store and exposes the React read hook. The whole
// module is pure client-side OBSERVABILITY — it measures where the user's
// wall-time goes during lag/freeze and stores it in memory; it holds no app
// logic and talks to no backend (M141: the frontend is a dumb render layer, and
// this is a measurement harness bolted to the side of it, not a feature).
//
// `initTelemetry()` is idempotent and called once from `main.tsx`; the dev-mode
// HUD reads live state via `useTelemetry()`.

import { useSyncExternalStore } from "react"
import { getSnapshot, subscribe, type TelemetrySnapshot } from "./store"
import { initWebVitals } from "./vitals"
import { initLongFrames } from "./frames"
import { initStallMonitor } from "./stall"
import { initHeartbeat } from "./heartbeat"

export { TelemetryProfiler } from "./profiler"
export { measure, measureAsync } from "./task"
export { reset as resetTelemetry } from "./store"
export type {
  TelemetryEvent,
  TelemetrySnapshot,
  VitalEvent,
  LoafEvent,
  LongTaskEvent,
  CommitEvent,
  StallEvent,
  TaskEvent,
  BlockEvent,
  TaskAgg,
} from "./store"

// Idempotency flag on a const holder — set by property assignment (never a
// top-level `let` reassignment) per unicorn/no-top-level-assignment-in-function.
const state = { started: false }

/**
 * Arm all telemetry producers exactly once. Safe to call repeatedly (StrictMode
 * double-invokes effects, HMR re-runs modules) — subsequent calls no-op.
 */
export function initTelemetry(): void {
  if (state.started) return
  state.started = true
  initWebVitals()
  initLongFrames()
  initStallMonitor()
  initHeartbeat()
}

/** Subscribe a component to the live telemetry snapshot. */
export function useTelemetry(): TelemetrySnapshot {
  return useSyncExternalStore(subscribe, getSnapshot)
}
