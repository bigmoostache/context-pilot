// ── Long Animation Frames + Long Tasks producer ──────────────────────
//
// The single most useful native signal for "what froze the UI": the **Long
// Animation Frames API** (LoAF, GA in Chromium 123+). Where the older Long
// Tasks API only says "a task ran >50ms", LoAF reports each janky *frame* with
// its `blockingDuration` AND a `scripts[]` array naming the exact functions and
// source files that consumed it — git-blame for jank. Crucially it fires for
// ALL long frames, including those with no user interaction: a background
// render storm (e.g. an SSE-delta flood re-rendering the tree) shows up here
// even though INP never would.
//
// We observe LoAF where supported and fall back to Long Tasks otherwise, so
// every browser yields *some* main-thread-blocking signal.
//
// The LoAF/Long-Task entry shapes are not yet in the TS DOM lib, so we declare
// the minimal structural subset we read (a downcast from `PerformanceEntry`, a
// supertype — no `any`, no unsafe bridge).

import { record } from "./store"

/** One script within a Long Animation Frame (subset of PerformanceScriptTiming). */
interface LoafScript {
  sourceURL?: string
  sourceFunctionName?: string
  duration: number
}

/** A LoAF entry (subset of PerformanceLongAnimationFrameTiming). */
interface LoafEntry extends PerformanceEntry {
  duration: number
  blockingDuration?: number
  scripts?: LoafScript[]
}

/** A Long Task attribution container (subset of TaskAttributionTiming). */
interface TaskContainer {
  containerType?: string
  containerName?: string
  containerId?: string
}

/** A Long Task entry (subset of PerformanceLongTaskTiming). */
interface LongTaskEntry extends PerformanceEntry {
  duration: number
  attribution?: TaskContainer[]
}

/** Format a LoAF's longest script as "func @ basename(source)". */
function culprit(scripts: LoafScript[] | undefined): string | undefined {
  if (!scripts || scripts.length === 0) return undefined
  const [longest] = [...scripts].toSorted((a, b) => b.duration - a.duration)
  if (!longest) return undefined
  const src = longest.sourceURL ? longest.sourceURL.split("/").pop() : undefined
  const fn = longest.sourceFunctionName || "(anonymous)"
  return src ? `${fn} @ ${src}` : fn
}

/** Format a Long Task attribution container, when present. */
function container(attribution: TaskContainer[] | undefined): string | undefined {
  const first = attribution?.[0]
  if (!first) return undefined
  const id = first.containerName ?? first.containerId
  return id ? `${first.containerType ?? "frame"}:${id}` : first.containerType
}

/** True when the browser can observe the given PerformanceObserver entry type. */
function supports(type: string): boolean {
  if (typeof PerformanceObserver === "undefined") return false
  const types = PerformanceObserver.supportedEntryTypes
  return Array.isArray(types) && types.includes(type)
}

/** Observe an entry type, routing each entry through `onEntry`. Returns a
 *  disposer, or null when the type is unsupported / the observer throws. */
function observe(type: string, onEntry: (e: PerformanceEntry) => void): (() => void) | null {
  if (!supports(type)) return null
  try {
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) onEntry(entry)
    })
    observer.observe({ type, buffered: true })
    return () => observer.disconnect()
  } catch {
    // A browser that lists the type but rejects the options — treat as absent.
    return null
  }
}

/**
 * Start observing main-thread-blocking frames/tasks. Prefers LoAF (rich script
 * attribution); always also arms Long Tasks as the broadly-supported floor.
 * Returns a disposer that tears down every active observer.
 */
export function initLongFrames(): () => void {
  const disposers: (() => void)[] = []

  const loaf = observe("long-animation-frame", (entry) => {
    const e = entry as LoafEntry
    record({
      kind: "loaf",
      duration: Math.round(e.duration),
      blockingDuration: Math.round(e.blockingDuration ?? 0),
      script: culprit(e.scripts),
      ts: Date.now(),
    })
  })
  if (loaf) disposers.push(loaf)

  const longtask = observe("longtask", (entry) => {
    const e = entry as LongTaskEntry
    record({
      kind: "longtask",
      duration: Math.round(e.duration),
      container: container(e.attribution),
      ts: Date.now(),
    })
  })
  if (longtask) disposers.push(longtask)

  return () => {
    for (const dispose of disposers) dispose()
  }
}
