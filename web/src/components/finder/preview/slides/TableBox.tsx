import type { CSSProperties } from "react"
import type { SlideStroke, SlideTable, SlideTableCell } from "@/lib/api"
import { anchorJustify, px } from "./units"
import { TextBox } from "./TextBox"

/** One cell border as CSS, hidden when absent. */
function border(stroke: SlideStroke | null | undefined, scale: number): string | undefined {
  if (!stroke) return undefined
  const style = stroke.dash ? "dashed" : "solid"
  return `${Math.max(px(stroke.width, scale), 0.5)}px ${style} ${stroke.color}`
}

/** A DrawingML table: fixed column widths, row heights, spans, per-side borders. */
export function TableBox({ table, scale }: { table: SlideTable; scale: number }) {
  return (
    <table
      style={{ borderCollapse: "collapse", tableLayout: "fixed", position: "absolute", inset: 0 }}
    >
      <colgroup>
        {table.cols.map((width, i) => (
          <col key={i} style={{ width: px(width, scale) }} />
        ))}
      </colgroup>
      <tbody>
        {table.rows.map((row, ri) => (
          <tr key={ri} style={{ height: px(row.height, scale) }}>
            {row.cells.map((cell, ci) =>
              cell.merged ? null : <Cell key={ci} cell={cell} scale={scale} />,
            )}
          </tr>
        ))}
      </tbody>
    </table>
  )
}

/** One cell, its text laid out like a shape's. */
function Cell({ cell, scale }: { cell: SlideTableCell; scale: number }) {
  const [left, top, right, bottom] = cell.borders
  const style: CSSProperties = {
    position: "relative",
    padding: 0,
    background: cell.fill,
    borderLeft: border(left, scale),
    borderTop: border(top, scale),
    borderRight: border(right, scale),
    borderBottom: border(bottom, scale),
    verticalAlign:
      anchorJustify(cell.text.anchor) === "center"
        ? "middle"
        : cell.text.anchor === "b"
          ? "bottom"
          : "top",
  }
  return (
    <td colSpan={Math.max(cell.colSpan, 1)} rowSpan={Math.max(cell.rowSpan, 1)} style={style}>
      {/* The text box is absolutely inset; keep the cell's own row height. */}
      <div style={{ position: "relative", minHeight: "100%", height: "100%" }}>
        <TextBox body={cell.text} scale={scale} />
      </div>
    </td>
  )
}
