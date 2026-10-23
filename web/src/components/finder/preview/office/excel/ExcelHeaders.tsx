import type { CSSProperties, ReactNode } from "react"
import { EXCEL } from "./excelTheme"

/** Excel's header strip sizes at 100% zoom: column band height, row gutter width. */
const COL_H = 30
const ROW_W = 43
const HEADER_INK = "#5e5e5e"
const HEADER_LINE = "#e1e1e1"
/** Top of the engine's scroll host: ExcelToolbar (ribbon + formula row) plus the column band. */
const GRID_TOP = EXCEL.ribbonH + EXCEL.formulaH + COL_H

/**
 * The part of xlsx-react's `XlsxViewport` (vendor patch 0003) the headers read.
 * Structural, so the file type-checks against builds that predate the export.
 */
export interface HeaderViewport {
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
  } | null
}

/**
 * Wraps an XlsxEditor mounted with `toolbar={<ExcelToolbar />}` and paints
 * Excel's row and column headers, which the engine does not draw. Its scroll
 * host is inset by the header sizes; the strips are laid out from the last
 * painted frame (`onViewport`). Offsets are in viewport space: the canvas is
 * sticky, scrolling only changes which tracks are listed.
 */
export function ExcelHeaderFrame({
  viewport,
  children,
}: {
  viewport: HeaderViewport | null
  children: ReactNode
}) {
  return (
    <div className="excel-grid-frame" style={{ position: "relative", height: "100%" }}>
      <style>{`.excel-grid-frame [data-testid="xlsx-scroll"]{margin:${String(COL_H)}px 0 0 ${String(ROW_W)}px}`}</style>
      {children}
      {viewport?.grid && <Headers viewport={viewport} grid={viewport.grid} />}
    </div>
  )
}

function Headers({
  viewport,
  grid,
}: {
  viewport: HeaderViewport
  grid: NonNullable<HeaderViewport["grid"]>
}) {
  const z = viewport.zoom
  const cols = spans(grid.colOffsets, viewport.width / z)
  const rows = spans(grid.rowOffsets, viewport.height / z)
  const top = GRID_TOP - COL_H
  return (
    <div aria-hidden style={{ pointerEvents: "none", fontFamily: EXCEL.font }}>
      <div style={{ ...strip, top, left: 0, width: ROW_W, height: COL_H }} />
      <div style={{ ...strip, top, left: ROW_W, width: viewport.width, height: COL_H }}>
        {cols.map(([start, end], i) => (
          <span key={i} style={cell({ x: start * z, y: 0, w: (end - start) * z, h: COL_H }, "col")}>
            {columnName(grid.colIndices?.[i] ?? grid.startCol + i)}
          </span>
        ))}
      </div>
      <div style={{ ...strip, top: GRID_TOP, left: 0, width: ROW_W, height: viewport.height }}>
        {rows.map(([start, end], i) => (
          <span key={i} style={cell({ x: 0, y: start * z, w: ROW_W, h: (end - start) * z }, "row")}>
            {(grid.rowIndices?.[i] ?? grid.startRow + i) + 1}
          </span>
        ))}
      </div>
    </div>
  )
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
