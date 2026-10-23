import type { CSSProperties } from "react"
import { EXCEL } from "./excelTheme"
import {
  LEVEL_PX,
  isCollapsed,
  maxLevel,
  outlineGroups,
  showLevelBatches,
  toggleBatch,
  type OutlineGroup,
  type OutlineRuns,
  type Spans,
  type VisibilityBatch,
} from "./outline"

const BOX = 13
const LINE = "#7a7a7a"
const INK = "#424242"

export type Axis = "rows" | "cols"

/** One axis of the engine's grid meta, as the gutter reads it. */
export interface AxisMeta {
  runs: OutlineRuns | undefined
  hidden: Spans | undefined
  summaryBefore: boolean
  start: number
  indices: number[] | undefined
  offsets: number[]
}

/** Visible `[start, end]` pixel span (zoom applied) per visible index. */
function visibleSpans(meta: AxisMeta, zoom: number, limit: number): Map<number, [number, number]> {
  const out = new Map<number, [number, number]>()
  for (let i = 0; i + 1 < meta.offsets.length; i++) {
    const a = (meta.offsets[i] ?? 0) * zoom
    if (a >= limit) break
    const b = (meta.offsets[i + 1] ?? 0) * zoom
    if (b > a) out.set(meta.indices?.[i] ?? meta.start + i, [a, b])
  }
  return out
}

/** Pixel extent of the group's visible members, or null when none is painted. */
function groupExtent(
  g: OutlineGroup,
  spans: Map<number, [number, number]>,
): [number, number] | null {
  let lo = Infinity
  let hi = -Infinity
  for (const [index, [a, b]] of spans) {
    if (index < g.first || index > g.last) continue
    lo = Math.min(lo, a)
    hi = Math.max(hi, b)
  }
  return lo < hi ? [lo, hi] : null
}

/** `[along, across, length, thickness]` of a gutter mark. */
type Slot = [number, number, number, number]

/** Swap x/y for the column axis, so one layout serves both. */
function place(axis: Axis, [along, across, len, thick]: Slot): CSSProperties {
  return axis === "rows"
    ? { position: "absolute", top: along, left: across, height: len, width: thick }
    : { position: "absolute", left: along, top: across, width: len, height: thick }
}

/**
 * One outline gutter strip (left of the row headers, or above the column
 * headers): a bracket per expanded group ending at its summary, a +/- button on
 * each painted summary. Clicks emit visibility batches.
 */
export function OutlineStrip({
  axis,
  meta,
  zoom,
  extent,
  style,
  onBatch,
}: {
  axis: Axis
  meta: AxisMeta
  zoom: number
  extent: number
  style: CSSProperties
  onBatch: (batch: VisibilityBatch) => void
}) {
  const spans = visibleSpans(meta, zoom, extent)
  const groups = outlineGroups(meta.runs, meta.summaryBefore)
  return (
    <div style={{ position: "absolute", overflow: "hidden", background: EXCEL.chrome, ...style }}>
      {groups.map((g) => {
        const collapsed = isCollapsed(g, meta.hidden)
        const across = (g.level - 1) * LEVEL_PX + (LEVEL_PX - BOX) / 2
        const mid = across + BOX / 2
        const summary = spans.get(g.summary)
        const body = collapsed ? null : groupExtent(g, spans)
        const key = `${String(g.level)}-${String(g.first)}`
        return (
          <span key={key}>
            {body && (
              <span
                style={{
                  ...place(axis, [body[0] + 2, mid, body[1] - body[0] - 4, 1]),
                  background: LINE,
                }}
              />
            )}
            {summary && (
              <button
                type="button"
                aria-label={`${collapsed ? "Expand" : "Collapse"} group ${String(g.first + 1)}–${String(g.last + 1)}`}
                aria-expanded={!collapsed}
                onClick={() => {
                  onBatch(toggleBatch(g, meta.runs, meta.hidden))
                }}
                style={{
                  ...place(axis, [(summary[0] + summary[1] - BOX) / 2, across, BOX, BOX]),
                  ...boxStyle,
                }}
              >
                {collapsed ? "+" : "−"}
              </button>
            )}
          </span>
        )
      })}
    </div>
  )
}

/** The 1, 2, 3… buttons: show the outline down to that level. */
export function LevelButtons({
  axis,
  meta,
  style,
  onBatch,
}: {
  axis: Axis
  meta: AxisMeta
  style: CSSProperties
  onBatch: (batch: VisibilityBatch) => void
}) {
  const count = maxLevel(meta.runs) + 1
  return (
    <div style={{ position: "absolute", ...style }}>
      {Array.from({ length: count }, (_, i) => (
        <button
          key={i}
          type="button"
          aria-label={`Show ${axis === "rows" ? "row" : "column"} outline level ${String(i + 1)}`}
          onClick={() => {
            const batches = showLevelBatches(meta.runs, meta.hidden, i + 1)
            for (const batch of batches) onBatch(batch)
          }}
          style={{
            ...place(axis === "rows" ? "cols" : "rows", [i * LEVEL_PX + 1, 0, BOX, BOX]),
            ...boxStyle,
          }}
        >
          {i + 1}
        </button>
      ))}
    </div>
  )
}

const boxStyle: CSSProperties = {
  boxSizing: "border-box",
  padding: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  border: `1px solid ${LINE}`,
  borderRadius: 2,
  background: "#ffffff",
  color: INK,
  fontSize: 10,
  lineHeight: 1,
  fontFamily: EXCEL.font,
  cursor: "pointer",
}
