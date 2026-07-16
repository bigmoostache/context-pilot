import type { IWorkbookData, ICellData, IObjectMatrixPrimitiveType } from "@univerjs/presets"
import ExcelJS from "exceljs"
import { postApiAgentByIdFsUpload } from "@/lib/api/generated"

/** Backend sheet shape from the calamine `/fs/sheet` endpoint. */
export interface SheetPayload {
  name: string
  rows: string[][]
  /** Per-cell formula strings (parallel to rows). Null = no formula. */
  formulas?: (string | null)[][]
}

/**
 * Convert calamine JSON rows into Univer's IWorkbookData format.
 * Each sheet becomes a keyed entry; cell values are auto-typed
 * (number when parseable, string otherwise).
 */
export function toWorkbookData(sheets: SheetPayload[]): IWorkbookData {
  const sheetMap: Record<string, Record<string, unknown>> = {}
  const sheetOrder: string[] = []

  for (const [idx, sheet] of sheets.entries()) {
    const id = `s${idx}`
    sheetOrder.push(id)

    const rows = sheet.rows
    const rowCount = Math.max(rows.length, 100)
    const colCount = Math.max(
      rows.reduce((max, r) => Math.max(max, r.length), 0),
      26,
    )

    const cellData: IObjectMatrixPrimitiveType<ICellData> = {}
    for (let ri = 0; ri < rows.length; ri++) {
      const row = rows[ri]
      if (!row) continue
      const formulaRow = sheet.formulas?.[ri]
      const rowCells: Record<number, ICellData> = {}
      for (let ci = 0; ci < row.length; ci++) {
        const raw = row[ci]
        if (raw === undefined || raw === "") continue
        const num = Number(raw)
        const cell: ICellData = { v: !Number.isNaN(num) && raw.trim() !== "" ? num : raw }
        // Attach formula string from backend (calamine) if present.
        const formula = formulaRow?.[ci]
        if (formula) {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Univer's ICellData typing doesn't expose `f` but the engine reads it.
          ;(cell as Record<string, unknown>)["f"] = formula
        }
        rowCells[ci] = cell
      }
      cellData[ri] = rowCells
    }

    sheetMap[id] = {
      id,
      name: sheet.name,
      rowCount,
      columnCount: colCount,
      cellData,
    }
  }

  return {
    id: "workbook",
    sheetOrder,
    sheets: sheetMap,
  } as IWorkbookData
}

/**
 * Read current Univer workbook state and build an XLSX blob via ExcelJS.
 * Handles value + style mapping (bold, italic, underline, strike, text color,
 * background color, font size, horizontal alignment).
 */
export async function generateXlsxBlob(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  univerAPI: any,
): Promise<Blob | null> {
  const workbook = univerAPI.getActiveWorkbook()
  if (!workbook) return null

  const snapshot = workbook.getSnapshot()
  const wb = new ExcelJS.Workbook()

  for (const sheetId of snapshot.sheetOrder ?? []) {
    const sheetData = snapshot.sheets?.[sheetId]
    if (!sheetData) continue

    const ws = wb.addWorksheet(sheetData.name ?? sheetId)
    const cellData = sheetData.cellData ?? {}

    // Find actual data bounds.
    let maxRow = 0
    let maxCol = 0
    for (const ri of Object.keys(cellData)) {
      const row = Number(ri)
      if (row > maxRow) maxRow = row
      const cols = cellData[ri] as Record<string, unknown> | undefined
      if (!cols) continue
      for (const ci of Object.keys(cols)) {
        const col = Number(ci)
        if (col > maxCol) maxCol = col
      }
    }

    // Write cells.
    for (let ri = 0; ri <= maxRow; ri++) {
      const rowCells = cellData[ri] as Record<string, { v?: unknown; s?: unknown }> | undefined
      if (!rowCells) continue
      const wsRow = ws.getRow(ri + 1)
      for (let ci = 0; ci <= maxCol; ci++) {
        const cell = rowCells[ci]
        if (!cell) continue
        const wsCell = wsRow.getCell(ci + 1)

        // Write formula if present — ExcelJS CellFormulaValue carries both
        // the formula string and the cached result so Excel shows the value
        // immediately without recalc.
        const formula = (cell as Record<string, unknown>)["f"] as string | undefined
        if (formula) {
          wsCell.value = {
            formula,
            result: cell.v as number | string | boolean | Date,
          } as ExcelJS.CellFormulaValue
        } else {
          wsCell.value = cell.v as ExcelJS.CellValue
        }

        // Resolve style — may be inline IStyleData or a string ID referencing
        // the workbook's styles registry.
        let style = cell.s as Record<string, unknown> | string | undefined
        if (typeof style === "string" && snapshot.styles) {
          style = snapshot.styles[style] as Record<string, unknown> | undefined
        }
        if (style && typeof style === "object") {
          applyStyle(wsCell, style)
        }
      }
    }
  }

  const buffer = await wb.xlsx.writeBuffer()
  return new Blob([buffer], {
    type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  })
}

/**
 * Generate a modified XLSX from Univer state and trigger a browser download.
 */
export async function exportToXlsx(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  univerAPI: any,
  filename: string,
): Promise<void> {
  const blob = await generateXlsxBlob(univerAPI)
  if (!blob) return

  const url = URL.createObjectURL(blob)
  const a = document.createElement("a")
  a.href = url
  a.download = filename.replace(/\.[^.]+$/, "") + ".xlsx"
  a.click()
  URL.revokeObjectURL(url)
}

/**
 * Generate a modified XLSX from Univer state and upload it back to the agent
 * realm, overwriting the original file. The realm's file becomes the new
 * source of truth — the FinderPreview Download button then serves the
 * updated version.
 */
export async function saveToRealm(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  univerAPI: any,
  agentId: string,
  realmPath: string,
): Promise<void> {
  const blob = await generateXlsxBlob(univerAPI)
  if (!blob) throw new Error("No active workbook")

  const lastSlash = realmPath.lastIndexOf("/")
  const dir = lastSlash > 0 ? realmPath.slice(0, lastSlash) : ""
  const name = lastSlash >= 0 ? realmPath.slice(lastSlash + 1) : realmPath

  await postApiAgentByIdFsUpload({
    path: { id: agentId },
    query: { path: dir, name },
    body: blob,
  })
}

/** Map Univer style object to ExcelJS cell styling. */
function applyStyle(cell: ExcelJS.Cell, style: Record<string, unknown>): void {
  const font: Partial<ExcelJS.Font> = {}
  if (style["bl"] === 1) font.bold = true
  if (style["it"] === 1) font.italic = true

  // Univer stores underline/strike as ITextDecoration {s: BooleanNumber}.
  const ul = style["ul"] as { s?: number } | number | undefined
  if (typeof ul === "object" ? ul?.s === 1 : ul === 1) font.underline = true
  const st = style["st"] as { s?: number } | number | undefined
  if (typeof st === "object" ? st?.s === 1 : st === 1) font.strike = true

  // Text color — Univer IColorStyle: {rgb: 'RRGGBB'} or plain string fallback.
  const cl = style["cl"] as { rgb?: string } | string | undefined
  const textRgb = typeof cl === "object" ? cl?.rgb : typeof cl === "string" ? cl : undefined
  if (textRgb) font.color = { argb: textRgb.replace(/^#/, "") }

  if (style["fs"]) font.size = style["fs"] as number
  if (Object.keys(font).length > 0) cell.font = font

  // Background color — Univer IColorStyle on "bg" key.
  const bg = style["bg"] as { rgb?: string } | string | undefined
  const bgRgb = typeof bg === "object" ? bg?.rgb : typeof bg === "string" ? bg : undefined
  if (bgRgb) {
    cell.fill = {
      type: "pattern",
      pattern: "solid",
      fgColor: { argb: bgRgb.replace(/^#/, "") },
    }
  }

  // Horizontal alignment.
  const ht = style["ht"] as number | undefined
  if (ht !== undefined) {
    const map: Record<number, ExcelJS.Alignment["horizontal"]> = {
      0: "left",
      1: "center",
      2: "right",
    }
    cell.alignment = { horizontal: map[ht] ?? "left" }
  }

  // Borders — Univer IBorderData with t/b/l/r edges, each {s: BorderStyleTypes, cl: IColorStyle}.
  const bd = style["bd"] as Record<string, { s?: number; cl?: { rgb?: string } | string }> | undefined
  if (bd) {
    const border: Partial<ExcelJS.Borders> = {}
    const edgeMap: Record<string, keyof ExcelJS.Borders> = {
      t: "top",
      b: "bottom",
      l: "left",
      r: "right",
    }
    for (const [key, excelKey] of Object.entries(edgeMap)) {
      const edge = bd[key]
      if (!edge || !edge.s) continue
      const borderStyle = toBorderStyle(edge.s)
      if (!borderStyle) continue
      const edgeCl = edge.cl
      const edgeRgb = typeof edgeCl === "object" ? edgeCl?.rgb : typeof edgeCl === "string" ? edgeCl : undefined
      border[excelKey] = {
        style: borderStyle,
        color: edgeRgb ? { argb: edgeRgb.replace(/^#/, "") } : { argb: "000000" },
      }
    }
    if (Object.keys(border).length > 0) cell.border = border
  }
}

/** Map Univer BorderStyleTypes enum to ExcelJS border style string. */
function toBorderStyle(s: number): ExcelJS.BorderStyle | undefined {
  const map: Record<number, ExcelJS.BorderStyle> = {
    1: "thin",
    2: "hair",
    3: "dotted",
    4: "dashed",
    5: "dashDot",
    6: "dashDotDot",
    7: "double",
    8: "medium",
    9: "mediumDashed",
    10: "mediumDashDot",
    11: "mediumDashDotDot",
    12: "slantDashDot",
    13: "thick",
  }
  return map[s]
}
