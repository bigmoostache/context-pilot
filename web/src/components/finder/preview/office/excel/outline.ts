/**
 * Excel outline (row/column grouping) geometry over the engine's grid meta
 * (vendor patch 0007): groups from the level runs, their collapsed state from
 * the hidden spans, and the index batches that expand, collapse or show a level.
 * Collapse/expand are plain visibility batches, so nested restore is lossy: an
 * inner group open before its outer one collapsed comes back collapsed.
 */

/** `[first, last, level]` runs of equal outline level, sorted, non-overlapping. */
export type OutlineRuns = [number, number, number][]
/** Inclusive `[first, last]` hidden spans, sorted. */
export type Spans = [number, number][]

export interface OutlineGroup {
  level: number
  first: number
  last: number
  /** The row/col holding the +/- button; just outside the group. */
  summary: number
}

/** Pixel size of one outline level column (rows) or band (columns). */
export const LEVEL_PX = 16

/** Deepest outline level in `runs` (0: no outline). */
export function maxLevel(runs: OutlineRuns | undefined): number {
  return (runs ?? []).reduce((max, run) => Math.max(max, run[2]), 0)
}

/** Gutter extent for an axis: one band per level button (max + 1), or 0. */
export function gutterSize(runs: OutlineRuns | undefined): number {
  const max = maxLevel(runs)
  return max === 0 ? 0 : (max + 1) * LEVEL_PX
}

/**
 * Every group: for each level L, the maximal index ranges whose level is at
 * least L. Runs separated by a gap of ungrouped indices split a group.
 */
export function outlineGroups(
  runs: OutlineRuns | undefined,
  summaryBefore: boolean,
): OutlineGroup[] {
  const sorted = runs ?? []
  const groups: OutlineGroup[] = []
  for (let level = 1; level <= maxLevel(sorted); level++) {
    for (const [first, last] of rangesAtLevel(sorted, level)) {
      const summary = summaryBefore ? first - 1 : last + 1
      groups.push({ level, first, last, summary })
    }
  }
  return groups
}

/** Maximal contiguous `[first, last]` ranges of runs at `level` or deeper. */
function rangesAtLevel(runs: OutlineRuns, level: number): Spans {
  const out: Spans = []
  for (const [first, last, runLevel] of runs) {
    if (runLevel < level) continue
    const prev = out.at(-1)
    if (prev && prev[1] + 1 === first) prev[1] = last
    else out.push([first, last])
  }
  return out
}

/** Number of indices of `first..=last` covered by `spans`. */
function hiddenCount(spans: Spans | undefined, first: number, last: number): number {
  let count = 0
  const list = spans ?? []
  for (const [a, b] of list) {
    if (b < first) continue
    if (a > last) break
    count += Math.min(b, last) - Math.max(a, first) + 1
  }
  return count
}

/** Is `index` inside one of the hidden `spans`? */
export function isHidden(spans: Spans | undefined, index: number): boolean {
  return hiddenCount(spans, index, index) === 1
}

/** A group is collapsed when every index in it is hidden. */
export function isCollapsed(group: OutlineGroup, spans: Spans | undefined): boolean {
  return hiddenCount(spans, group.first, group.last) === group.last - group.first + 1
}

/** Indices of `runs` inside `first..=last` whose level passes `keep`. */
function indicesWhere(
  runs: OutlineRuns | undefined,
  first: number,
  last: number,
  keep: (level: number) => boolean,
): number[] {
  const out: number[] = []
  const list = runs ?? []
  for (const [a, b, level] of list) {
    if (b < first || a > last || !keep(level)) continue
    for (let i = Math.max(a, first); i <= Math.min(b, last); i++) out.push(i)
  }
  return out
}

/** A visibility batch: `indices` all get `hidden`. */
export interface VisibilityBatch {
  indices: number[]
  hidden: boolean
}

/**
 * Collapse hides the whole group; expand shows its own level (direct rows plus
 * inner groups' summaries), leaving inner groups collapsed.
 */
export function toggleBatch(
  group: OutlineGroup,
  runs: OutlineRuns | undefined,
  spans: Spans | undefined,
): VisibilityBatch {
  if (isCollapsed(group, spans)) {
    return {
      indices: indicesWhere(runs, group.first, group.last, (level) => level === group.level),
      hidden: false,
    }
  }
  return { indices: indicesWhere(runs, group.first, group.last, () => true), hidden: true }
}

/**
 * Level button `n` (1-based): indices below level `n` shown, the rest hidden.
 * Only indices whose state changes are listed; empty batches are dropped.
 */
export function showLevelBatches(
  runs: OutlineRuns | undefined,
  spans: Spans | undefined,
  n: number,
): VisibilityBatch[] {
  const all = indicesWhere(runs, 0, Number.MAX_SAFE_INTEGER, () => true)
  const level = new Map<number, number>()
  const list = runs ?? []
  for (const [a, b, l] of list) for (let i = a; i <= b; i++) level.set(i, l)
  const hide = all.filter((i) => (level.get(i) ?? 0) >= n && !isHidden(spans, i))
  const show = all.filter((i) => (level.get(i) ?? 0) < n && isHidden(spans, i))
  return [
    { indices: show, hidden: false },
    { indices: hide, hidden: true },
  ].filter((batch) => batch.indices.length > 0)
}
