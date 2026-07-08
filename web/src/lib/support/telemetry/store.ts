// ── Frontend performance telemetry — the in-memory sink ──────────────
//
// A tiny observable store (the `useSyncExternalStore` contract: `subscribe`
// + `getSnapshot`) that collects the raw signals answering ONE question —
// *where does the user's wall-time go when the cockpit lags?* It holds a
// bounded ring buffer of recent events plus a few live aggregates, and nothing
// else: no business logic, no network, no vendor SDK (M141 — the frontend is a
// dumb render/observability layer). The three producers (web-vitals, Long
// Animation Frames, the React Profiler) all funnel their normalized events
// through {@link record}; the dev-mode HUD and the console reporter read the
// snapshot.
//
// Notifications are COALESCED behind a timer so a burst of events (exactly the
// pathology we're hunting — an SSE-driven render storm) can't make the HUD
// re-render per event and pollute the very measurements it displays.
//
// All mutable module state lives on ONE `const store` holder and is mutated by
// PROPERTY assignment (never by reassigning a top-level binding), which is what
// unicorn/no-top-level-assignment-in-function requires — the observable-store
// singleton expressed without a mutable module-level `let`.

/** Discriminated telemetry event union — one variant per producer. */
export type TelemetryEvent =
  VitalEvent | LoafEvent | LongTaskEvent | CommitEvent | StallEvent | TaskEvent | BlockEvent

/** A Core Web Vital sample (INP/LCP/CLS/FCP/TTFB) from the web-vitals lib. */
export interface VitalEvent {
  kind: "vital"
  /** Metric acronym, e.g. "INP". */
  name: string
  /** Metric value in ms (or unitless for CLS). */
  value: number
  /** web-vitals rating bucket. */
  rating: "good" | "needs-improvement" | "poor"
  /** One-line attribution (slowest phase / target element), when available. */
  detail: string | undefined
  ts: number
}

/** A Long Animation Frame (LoAF) — a frame that blocked the main thread. */
export interface LoafEvent {
  kind: "loaf"
  /** Total frame duration in ms. */
  duration: number
  /** Portion that blocked input (the freeze-perceptible part) in ms. */
  blockingDuration: number
  /** Best-guess culprit: "func @ source.js" of the frame's longest script. */
  script: string | undefined
  ts: number
}

/** A Long Task (>50ms) — the coarser, wider-support fallback to LoAF. */
export interface LongTaskEvent {
  kind: "longtask"
  duration: number
  /** Attribution container (e.g. "iframe#..."), when the browser provides it. */
  container: string | undefined
  ts: number
}

/** A React commit whose render work crossed the reporting threshold. */
export interface CommitEvent {
  kind: "commit"
  /** The <Profiler id> of the committed subtree. */
  id: string
  phase: "mount" | "update" | "nested-update"
  /** Actual render time for this commit in ms. */
  actualDuration: number
  ts: number
}

/** A main-thread stall — the gap between animation frames exceeded the stall
 *  threshold, i.e. the event loop was blocked (the universal freeze signal). */
export interface StallEvent {
  kind: "stall"
  /** Blocked duration in ms (≈ the perceived freeze length). */
  gap: number
  ts: number
}

/** A THROTTLE-IMMUNE main-thread block, from the Web-Worker heartbeat. The
 *  worker ticks on its own (never-throttled) thread; a large gap between the
 *  heartbeats the MAIN thread processed means the main thread was genuinely
 *  blocked. Unlike {@link StallEvent} (rAF-based, fooled by focus-throttling),
 *  a `block` is a REAL freeze — the authoritative headline signal. */
export interface BlockEvent {
  kind: "block"
  /** Blocked duration in ms (heartbeat gap minus the expected interval). */
  blocked: number
  ts: number
}

/** A NAMED synchronous main-thread block, timed by an explicit {@link measure}
 *  wrapper around a suspected hot path (SSE fold, poll reconcile, JSON parse…).
 *  This is the ATTRIBUTION layer for a stall: on browsers with no Long
 *  Animation Frames API (Firefox/Safari) the rAF stall watchdog says *a* block
 *  of N ms happened, and a matching `task` entry says *which labelled path*
 *  burned it. */
export interface TaskEvent {
  kind: "task"
  /** Stable label of the instrumented path, e.g. "sse:apply", "threads:merge". */
  label: string
  /** Synchronous wall-time the block took, in ms. */
  duration: number
  ts: number
}

/** Per-label task aggregation — count, summed and max duration for a
 *  {@link TaskEvent} label. This is the BURST-CATCHER: a storm of individually
 *  sub-threshold ops (e.g. hundreds of ~5ms SSE applies that never trip a
 *  single named-task entry) surfaces here as a huge `count`/`total` even though
 *  no single call was slow — the only way to name a death-by-a-thousand-cuts
 *  freeze. */
export interface TaskAgg {
  label: string
  count: number
  total: number
  max: number
}

/** The immutable snapshot handed to React via `useSyncExternalStore`. */
export interface TelemetrySnapshot {
  /** Most recent value per Web Vital, keyed by metric name. */
  vitals: Record<string, VitalEvent>
  /** The worst (longest) THROTTLE-IMMUNE main-thread blocks (worker heartbeat),
   *  descending — the AUTHORITATIVE freeze signal (a real block, never a
   *  focus-throttling artifact like a bare rAF stall can be). */
  worstBlocks: BlockEvent[]
  /** Number of real main-thread blocks observed by the worker heartbeat. */
  blockCount: number
  /** The worst (longest) main-thread stalls seen, descending — THE headline
   *  freeze signal (a 2s hang shows here even when nothing else fires). */
  worstStalls: StallEvent[]
  /** Number of stalls observed. */
  stallCount: number
  /** The worst (longest) NAMED tasks seen, descending — the attribution for a
   *  stall (which labelled hot path burned the main-thread time). */
  worstTasks: TaskEvent[]
  /** Number of named tasks recorded (≥ threshold). */
  taskCount: number
  /** Per-label task totals, descending by summed duration — the burst-catcher
   *  that reveals a storm of individually-cheap ops summing to a freeze. */
  taskAgg: TaskAgg[]
  /** The worst (longest) Long Animation Frames seen, descending. */
  worstFrames: LoafEvent[]
  /** The worst (longest) React commits seen, descending. */
  worstCommits: CommitEvent[]
  /** Total React commits recorded (≥ threshold) — a high count with small
   *  durations is the SSE render-storm signature (many cheap commits, not one
   *  expensive one). */
  commitCount: number
  /** Summed actual render time across all recorded commits (ms). */
  commitTotalMs: number
  /** Rolling count of Long Tasks observed. */
  longTaskCount: number
  /** Total blocking time attributed to Long Animation Frames (ms). */
  totalBlockingMs: number
  /** The most recent events, newest first (bounded ring buffer). */
  recent: TelemetryEvent[]
}

const RING_CAPACITY = 100
const LEADERBOARD_SIZE = 8
const NOTIFY_COALESCE_MS = 300
/** Task spans below this are aggregated (count/total/max) but NOT added to the
 *  worst-list/ring — keeps a burst of thousands of cheap ops out of the ring
 *  while still summing them in {@link TaskAgg} (the burst-catcher). */
const TASK_RING_MIN_MS = 5

function emptySnapshot(): TelemetrySnapshot {
  return {
    vitals: {},
    worstBlocks: [],
    blockCount: 0,
    worstStalls: [],
    stallCount: 0,
    worstTasks: [],
    taskCount: 0,
    taskAgg: [],
    worstFrames: [],
    worstCommits: [],
    commitCount: 0,
    commitTotalMs: 0,
    longTaskCount: 0,
    totalBlockingMs: 0,
    recent: [],
  }
}

// All mutable state on a single const holder — mutated by property assignment
// only. `snapshot` is the cached, stable reference `getSnapshot` returns until a
// coalesced flush rebuilds it (a fresh reference every flush, unchanged in
// between — the `useSyncExternalStore` stability contract).
const store = {
  listeners: new Set<() => void>(),
  ring: [] as TelemetryEvent[],
  vitals: {} as Record<string, VitalEvent>,
  worstBlocks: [] as BlockEvent[],
  blockCount: 0,
  worstStalls: [] as StallEvent[],
  stallCount: 0,
  worstTasks: [] as TaskEvent[],
  taskCount: 0,
  taskAgg: new Map<string, TaskAgg>(),
  worstFrames: [] as LoafEvent[],
  worstCommits: [] as CommitEvent[],
  commitCount: 0,
  commitTotalMs: 0,
  longTaskCount: 0,
  totalBlockingMs: 0,
  snapshot: emptySnapshot(),
  flushTimer: undefined as number | undefined,
}

/** Insert `item` into a descending "worst N" list keyed by `weight`. */
function rankInto<T>(list: T[], item: T, weight: (x: T) => number): T[] {
  return [...list, item].toSorted((a, b) => weight(b) - weight(a)).slice(0, LEADERBOARD_SIZE)
}

/** Record a telemetry event and schedule a coalesced listener notification. */
export function record(event: TelemetryEvent): void {
  store.ring.unshift(event)
  if (store.ring.length > RING_CAPACITY) store.ring.length = RING_CAPACITY

  switch (event.kind) {
    case "vital": {
      store.vitals[event.name] = event
      break
    }
    case "block": {
      store.worstBlocks = rankInto(store.worstBlocks, event, (b) => b.blocked)
      store.blockCount += 1
      break
    }
    case "stall": {
      store.worstStalls = rankInto(store.worstStalls, event, (s) => s.gap)
      store.stallCount += 1
      break
    }
    case "task": {
      // ALWAYS aggregate (the burst-catcher) — a storm of sub-threshold ops
      // must sum here even though none is individually ranked.
      const agg = store.taskAgg.get(event.label) ?? {
        label: event.label,
        count: 0,
        total: 0,
        max: 0,
      }
      agg.count += 1
      agg.total += event.duration
      if (event.duration > agg.max) agg.max = event.duration
      store.taskAgg.set(event.label, agg)
      store.taskCount += 1
      // Only rank/ring the ones big enough to matter individually.
      if (event.duration >= TASK_RING_MIN_MS) {
        store.worstTasks = rankInto(store.worstTasks, event, (t) => t.duration)
      }
      break
    }
    case "loaf": {
      store.worstFrames = rankInto(store.worstFrames, event, (f) => f.duration)
      store.totalBlockingMs += event.blockingDuration
      break
    }
    case "commit": {
      store.worstCommits = rankInto(store.worstCommits, event, (c) => c.actualDuration)
      store.commitCount += 1
      store.commitTotalMs += event.actualDuration
      break
    }
    case "longtask": {
      store.longTaskCount += 1
      break
    }
  }

  scheduleFlush()
}

/** Rebuild the cached snapshot after the coalescing window elapses. */
function scheduleFlush(): void {
  if (store.flushTimer !== undefined) return
  store.flushTimer = window.setTimeout(() => {
    store.flushTimer = undefined
    store.snapshot = {
      vitals: { ...store.vitals },
      worstBlocks: [...store.worstBlocks],
      blockCount: store.blockCount,
      worstStalls: [...store.worstStalls],
      stallCount: store.stallCount,
      worstTasks: [...store.worstTasks],
      taskCount: store.taskCount,
      taskAgg: store.taskAgg
        .values()
        .toArray()
        .toSorted((a, b) => b.total - a.total),
      worstFrames: [...store.worstFrames],
      worstCommits: [...store.worstCommits],
      commitCount: store.commitCount,
      commitTotalMs: store.commitTotalMs,
      longTaskCount: store.longTaskCount,
      totalBlockingMs: store.totalBlockingMs,
      recent: [...store.ring],
    }
    for (const listener of store.listeners) listener()
  }, NOTIFY_COALESCE_MS)
}

/** Subscribe to store changes (the `useSyncExternalStore` subscribe fn). */
export function subscribe(listener: () => void): () => void {
  store.listeners.add(listener)
  return () => store.listeners.delete(listener)
}

/** Current snapshot — a stable reference between coalesced flushes. */
export function getSnapshot(): TelemetrySnapshot {
  return store.snapshot
}

/** Wipe all collected telemetry (HUD "clear" affordance). */
export function reset(): void {
  store.ring.length = 0
  store.vitals = {}
  store.worstBlocks = []
  store.blockCount = 0
  store.worstStalls = []
  store.stallCount = 0
  store.worstTasks = []
  store.taskCount = 0
  store.taskAgg = new Map()
  store.worstFrames = []
  store.worstCommits = []
  store.commitCount = 0
  store.commitTotalMs = 0
  store.longTaskCount = 0
  store.totalBlockingMs = 0
  store.snapshot = emptySnapshot()
  for (const listener of store.listeners) listener()
}
