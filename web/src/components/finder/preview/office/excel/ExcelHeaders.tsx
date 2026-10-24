import { useState, type CSSProperties, type ReactNode } from "react"
import { EXCEL } from "./excelTheme"
import { LevelButtons, OutlineStrip, type Axis, type AxisMeta } from "./ExcelOutline"
import {
  applyBatch,
  gutterSize,
  type OutlineRuns,
  type Spans,
  type VisibilityBatch,
} from "./outline"

/** Excel's header strip sizes at 100% zoom: column band height, row gutter width. */
const COL_H = 30
const ROW_W = 43
const HEADER_INK = "#5e5e5e"
const HEADER_LINE = "#e1e1e1"
/** Top of the header bands: ExcelToolbar (ribbon + formula row) above them. */
const BAND_TOP = EXCEL.ribbonH + EXCEL.formulaH

/**
 * The part of xlsx-react's `XlsxViewport` (vendor patches 0003 + 0007) the
 * headers and outline gutters read. Structural, so the file type-checks
 * against builds that predate the export.
 */
export interface HeaderViewport {
  sheet: number
  width: number
  height: number
  zoom: number
  grid: {
    startRow: number
    startCol: number
    rowIndices?: number[]
    colIndices?: number[]
    rowOffsets: number[]
    colOffsets: number[]
    hiddenRowSpans?: Spans
    hiddenColSpans?: Spans
    rowOutline?: OutlineRuns
    colOutline?: OutlineRuns
    outlineSummaryAbove?: boolean
    outlineSummaryLeft?: boolean
  } | null
}

/** Applies one outline visibility batch on `sheet` (the `outlineVisibility` command). */
export type OutlineDispatch = (axis: Axis, sheet: number, batch: VisibilityBatch) => void

/**
 * Wraps an XlsxEditor mounted with `toolbar={<ExcelToolbar />}` and paints
 * Excel's row and column headers plus the outline (grouping) gutters, which the
 * engine does not draw. Its scroll host is inset by their sizes; the strips are
 * laid out from the last painted frame (`onViewport`). Offsets are in viewport
 * space: the canvas is sticky, scrolling only changes which tracks are listed.
 */
export function ExcelHeaderFrame({
  viewport,
  onOutline,
  children,
}: {
  viewport: HeaderViewport | null
  onOutline: OutlineDispatch
  children: ReactNode
}) {
  const [pending, setPending] = useState<Pending | null>(null)
  // A pending flip only holds until the engine paints its next frame.
  const live = pending?.viewport === viewport ? pending : null
  const grid = viewport?.grid && live ? { ...viewport.grid, ...live.spans } : viewport?.grid
  const rowGutter = gutterSize(grid?.rowOutline)
  const colGutter = gutterSize(grid?.colOutline)
  const dispatch: OutlineDispatch = (axis, sheet, batch) => {
    if (viewport && grid) {
      const key = axis === "rows" ? "hiddenRowSpans" : "hiddenColSpans"
      setPending({ viewport, spans: { ...live?.spans, [key]: applyBatch(grid[key], batch) } })
    }
    onOutline(axis, sheet, batch)
  }
  return (
    <div
      className="excel-grid-frame"
      aria-busy={live !== null}
      style={{ position: "relative", height: "100%", cursor: live ? "progress" : undefined }}
    >
      <style>{`.excel-grid-frame [data-testid="xlsx-scroll"]{margin:${String(COL_H + colGutter)}px 0 0 ${String(ROW_W + rowGutter)}px}`}</style>
      {children}
      {viewport && grid && (
        <Headers
          viewport={viewport}
          grid={grid}
          rowGutter={rowGutter}
          colGutter={colGutter}
          onOutline={dispatch}
        />
      )}
    </div>
  )
}

/**
 * Hidden spans the gutter shows before the engine repaints (a visibility op can
 * recalc for seconds), tied to the frame they were derived from.
 */
interface Pending {
  viewport: HeaderViewport
  spans: { hiddenRowSpans?: Spans; hiddenColSpans?: Spans }
}

type Grid = NonNullable<HeaderViewport["grid"]>

function Headers({
  viewport,
  grid,
  rowGutter,
  colGutter,
  onOutline,
}: {
  viewport: HeaderViewport
  grid: Grid
  rowGutter: number
  colGutter: number
  onOutline: OutlineDispatch
}) {
  const z = viewport.zoom
  const cols = spans(grid.colOffsets, viewport.width / z)
  const rows = spans(grid.rowOffsets, viewport.height / z)
  const colTop = BAND_TOP + colGutter
  const gridTop = colTop + COL_H
  const gridLeft = rowGutter + ROW_W
  const rowMeta = axisMeta(grid, "rows")
  const colMeta = axisMeta(grid, "cols")
  const dispatch = (axis: Axis) => (batch: VisibilityBatch) => {
    onOutline(axis, viewport.sheet, batch)
  }
  return (
    <div style={{ fontFamily: EXCEL.font }}>
      <div aria-hidden style={{ pointerEvents: "none" }}>
        <div
          style={{ ...strip, top: BAND_TOP, left: 0, width: gridLeft, height: colGutter + COL_H }}
        />
        <div
          style={{ ...strip, top: colTop, left: gridLeft, width: viewport.width, height: COL_H }}
        >
          {cols.map(([start, end], i) => (
            <span
              key={i}
              style={cell({ x: start * z, y: 0, w: (end - start) * z, h: COL_H }, "col")}
            >
              {columnName(grid.colIndices?.[i] ?? grid.startCol + i)}
            </span>
          ))}
        </div>
        <div
          style={{ ...strip, top: gridTop, left: rowGutter, width: ROW_W, height: viewport.height }}
        >
          {rows.map(([start, end], i) => (
            <span
              key={i}
              style={cell({ x: 0, y: start * z, w: ROW_W, h: (end - start) * z }, "row")}
            >
              {(grid.rowIndices?.[i] ?? grid.startRow + i) + 1}
            </span>
          ))}
        </div>
      </div>
      {rowGutter > 0 && (
        <>
          <OutlineStrip
            axis="rows"
            meta={rowMeta}
            zoom={z}
            extent={viewport.height}
            style={{ top: gridTop, left: 0, width: rowGutter, height: viewport.height }}
            onBatch={dispatch("rows")}
          />
          <LevelButtons
            axis="rows"
            meta={rowMeta}
            style={{ top: colTop + (COL_H - 13) / 2, left: 0 }}
            onBatch={dispatch("rows")}
          />
        </>
      )}
      {colGutter > 0 && (
        <>
          <OutlineStrip
            axis="cols"
            meta={colMeta}
            zoom={z}
            extent={viewport.width}
            style={{ top: BAND_TOP, left: gridLeft, width: viewport.width, height: colGutter }}
            onBatch={dispatch("cols")}
          />
          <LevelButtons
            axis="cols"
            meta={colMeta}
            style={{ top: BAND_TOP, left: rowGutter + (ROW_W - 13) / 2 }}
            onBatch={dispatch("cols")}
          />
        </>
      )}
    </div>
  )
}

/** One axis of the grid meta in the gutter's terms. */
function axisMeta(grid: Grid, axis: Axis): AxisMeta {
  return axis === "rows"
    ? {
        runs: grid.rowOutline,
        hidden: grid.hiddenRowSpans,
        summaryBefore: grid.outlineSummaryAbove ?? false,
        start: grid.startRow,
        indices: grid.rowIndices,
        offsets: grid.rowOffsets,
      }
    : {
        runs: grid.colOutline,
        hidden: grid.hiddenColSpans,
        summaryBefore: grid.outlineSummaryLeft ?? false,
        start: grid.startCol,
        indices: grid.colIndices,
        offsets: grid.colOffsets,
      }
}

/** Column letters as Excel writes them: 0 → A, 25 → Z, 26 → AA. */
function columnName(index: number): string {
  let name = ""
  for (let n = index + 1; n > 0; n = Math.floor((n - 1) / 26)) {
    name = String.fromCodePoint(65 + ((n - 1) % 26)) + name
  }
  return name
}

/** Consecutive offset pairs clipped to the visible extent. */
function spans(offsets: number[], limit: number): [number, number][] {
  const out: [number, number][] = []
  for (let i = 0; i + 1 < offsets.length; i++) {
    const start = offsets[i] ?? 0
    if (start >= limit) break
    const end = offsets[i + 1] ?? start
    if (end > start) out.push([start, end])
  }
  return out
}

const strip: CSSProperties = {
  position: "absolute",
  overflow: "hidden",
  background: EXCEL.chrome,
  boxSizing: "border-box",
}

interface Box {
  x: number
  y: number
  w: number
  h: number
}

function cell({ x, y, w, h }: Box, axis: "col" | "row"): CSSProperties {
  return {
    position: "absolute",
    left: x,
    top: y,
    width: w,
    height: h,
    boxSizing: "border-box",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    fontSize: 13.33,
    color: HEADER_INK,
    borderRight: `1px solid ${HEADER_LINE}`,
    borderBottom: `1px solid ${HEADER_LINE}`,
    ...(axis === "row" && { justifyContent: "flex-end", paddingRight: 6 }),
  }
}
