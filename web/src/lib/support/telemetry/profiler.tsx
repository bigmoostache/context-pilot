// ── React Profiler producer ──────────────────────────────────────────
//
// Attributes wall-time to REACT itself: React's built-in <Profiler> reports,
// for every commit of the wrapped subtree, how long the render work actually
// took (`actualDuration`) and whether it was a mount or an update. This is the
// third leg of the tripod — web-vitals says the interaction was slow, LoAF says
// a frame blocked and names the script, and the Profiler says *which React
// subtree re-rendered and for how long*. Wrapping the app's hot regions makes
// an SSE-driven re-render storm legible: you see the same <Profiler id> commit
// again and again with a fat actualDuration.
//
// Only commits at/above COMMIT_THRESHOLD_MS are recorded, so the store isn't
// flooded by the cheap commits that make up the vast majority of renders.
// <Profiler> onRender fires in development automatically (and in a production
// profiling build), which is exactly where we do this measuring — zero deps.

import { Profiler, type ProfilerOnRenderCallback, type ReactNode } from "react"
import { record } from "./store"

/** Commits faster than this (one 60fps frame ≈ 16.7ms) aren't worth recording. */
const COMMIT_THRESHOLD_MS = 8

const onRender: ProfilerOnRenderCallback = (id, phase, actualDuration) => {
  if (actualDuration < COMMIT_THRESHOLD_MS) return
  record({
    kind: "commit",
    id,
    phase,
    actualDuration: Math.round(actualDuration * 10) / 10,
    ts: Date.now(),
  })
}

/**
 * Wrap a subtree to record its slow React commits under `id`. Nesting several
 * (e.g. one per major surface) gives per-region attribution. A pass-through in
 * production non-profiling builds where <Profiler> is inert.
 */
export function TelemetryProfiler({ id, children }: { id: string; children: ReactNode }) {
  return (
    <Profiler id={id} onRender={onRender}>
      {children}
    </Profiler>
  )
}
