import type {
  FUniver,
  IWorkbookData,
  ICellData,
  IObjectMatrixPrimitiveType,
  IStyleData,
  IColorStyle,
  ITextDecoration,
  IBorderStyleData,
  IBorderData,
  Nullable,
} from "@univerjs/presets"
import { BooleanNumber } from "@univerjs/presets"
import ExcelJS from "exceljs"
import { uploadFile } from "@/lib/api"

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
  const sheetOrder: string[] = []
  const sheetEntries: [string, { id: string; name: string; rowCount: number; columnCount: number; cellData: IObjectMatrixPrimitiveType<ICellData> }][] = []

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
    for (const [ri, row] of rows.entries()) {
      const formulaRow = sheet.formulas?.[ri]
      const rowCells = buildRowCells(row, formulaRow)
      if (Object.keys(rowCells).length > 0) {
        cellData[ri] = rowCells
      }
    }

    sheetEntries.push([id, { id, name: sheet.name, rowCount, columnCount: colCount, cellData }])
  }

  return {
    id: "workbook",
    sheetOrder,
    sheets: Object.fromEntries(sheetEntries),
  } as IWorkbookData
}

/** Parse one source row into a sparse cell map. */
function buildRowCells(
  row: string[],
  formulaRow: (string | null)[] | undefined,
): Record<number, ICellData> {
  const result: Record<number, ICellData> = {}
  for (const [ci, raw] of row.entries()) {
    if (raw === "") continue
    const num = Number(raw)
    const cell: ICellData = { v: !Number.isNaN(num) && raw.trim() !== "" ? num : raw }
    const formula = formulaRow?.[ci]
    if (formula) cell.f = formula
    result[ci] = cell
  }
  return result
}

// ── Data bounds ─────────────────────────────────────────────────────

interface DataBounds {
  maxRow: number
  maxCol: number
}

/** Scan cell matrix for actual data extent. */
function findDataBounds(
  cellData: IObjectMatrixPrimitiveType<ICellData>,
): DataBounds {
  let maxRow = 0
  let maxCol = 0
  for (const [ri, cols] of Object.entries(cellData) as [string, Record<number, ICellData> | undefined][]) {
    const row = Number(ri)
    if (row > maxRow) maxRow = row
    if (!cols) continue
    for (const ci of Object.keys(cols)) {
      const col = Number(ci)
      if (col > maxCol) maxCol = col
    }
  }
  return { maxRow, maxCol }
}

// ── Cell writing ────────────────────────────────────────────────────

/** Write one sheet's cells to an ExcelJS worksheet. */
function writeCells(
  ws: ExcelJS.Worksheet,
  cellData: IObjectMatrixPrimitiveType<ICellData>,
  bounds: DataBounds,
  styles: Record<string, Nullable<IStyleData>> | undefined,
): void {
  for (let ri = 0; ri <= bounds.maxRow; ri++) {
    const rowCells = cellData[ri]
    if (!rowCells) continue
    const wsRow = ws.getRow(ri + 1)
    writeCellRow(wsRow, rowCells, bounds.maxCol, styles)
  }
}

/** Write a single row of cells. */
function writeCellRow(
  wsRow: ExcelJS.Row,
  rowCells: Record<number, ICellData>,
  maxCol: number,
  styles: Record<string, Nullable<IStyleData>> | undefined,
): void {
  for (let ci = 0; ci <= maxCol; ci++) {
    const cell = rowCells[ci]
    if (!cell) continue
    const wsCell = wsRow.getCell(ci + 1)
    writeCellValue(wsCell, cell)
    const resolved = resolveStyle(cell.s, styles)
    if (resolved) applyStyle(wsCell, resolved)
  }
}

/** Write cell value or formula. */
function writeCellValue(wsCell: ExcelJS.Cell, cell: ICellData): void {
  wsCell.value = cell.f
    ? ({ formula: cell.f, result: cell.v } as ExcelJS.CellFormulaValue)
    : (cell.v as ExcelJS.CellValue)
}

/** Resolve style — may be inline IStyleData or a string ID referencing the workbook's registry. */
function resolveStyle(
  style: ICellData["s"],
  styles: Record<string, Nullable<IStyleData>> | undefined,
): IStyleData | null {
  if (!style) return null
  if (typeof style === "string") {
    return styles?.[style] ?? null
  }
  return style
}

// ── XLSX generation ─────────────────────────────────────────────────

/**
 * Read current Univer workbook state and build an XLSX blob via ExcelJS.
 * Handles value + style mapping (bold, italic, underline, strike, text color,
 * background color, font size, horizontal alignment, borders).
 */
export async function generateXlsxBlob(univerAPI: FUniver): Promise<Blob | null> {
  const workbook = univerAPI.getActiveWorkbook()
  if (!workbook) return null

  const snapshot = workbook.save()
  const wb = new ExcelJS.Workbook()

  for (const sheetId of snapshot.sheetOrder) {
    const sheetData = snapshot.sheets[sheetId]
    if (!sheetData) continue
    const ws = wb.addWorksheet(sheetData.name ?? sheetId)
    const cellData = sheetData.cellData ?? {}
    const bounds = findDataBounds(cellData)
    writeCells(ws, cellData, bounds, snapshot.styles)
  }

  const buffer = await wb.xlsx.writeBuffer()
  return new Blob([buffer], {
    type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  })
}

/**
 * Generate a modified XLSX from Univer state and upload it back to the agent
 * realm, overwriting the original file. The realm's file becomes the new
 * source of truth.
 */
export async function saveToRealm(
  univerAPI: FUniver,
  agentId: string,
  realmPath: string,
): Promise<void> {
  const blob = await generateXlsxBlob(univerAPI)
  if (!blob) throw new Error("No active workbook")

  const lastSlash = realmPath.lastIndexOf("/")
  const hasSlash = lastSlash !== -1
  const dir = hasSlash ? realmPath.slice(0, lastSlash) : ""
  const name = hasSlash ? realmPath.slice(lastSlash + 1) : realmPath

  const file = new File([blob], name, { type: blob.type })
  await uploadFile(agentId, dir, file)
}

// ── Style mapping ───────────────────────────────────────────────────

/** Map Univer IStyleData to ExcelJS cell styling. */
function applyStyle(cell: ExcelJS.Cell, style: IStyleData): void {
  applyFont(cell, style)
  applyFill(cell, style)
  applyAlignment(cell, style)
  applyBorders(cell, style)
}

/** Extract font properties from Univer style. */
function applyFont(cell: ExcelJS.Cell, style: IStyleData): void {
  const font: Partial<ExcelJS.Font> = {}
  if (style.bl === BooleanNumber.TRUE) font.bold = true
  if (style.it === BooleanNumber.TRUE) font.italic = true

  if (isDecorationActive(style.ul)) font.underline = true
  if (isDecorationActive(style.st)) font.strike = true

  const textRgb = extractRgb(style.cl)
  if (textRgb) font.color = { argb: textRgb.replace(/^#/, "") }
  if (style.fs) font.size = style.fs

  if (Object.keys(font).length > 0) cell.font = font
}

/** Apply background fill from Univer style. */
function applyFill(cell: ExcelJS.Cell, style: IStyleData): void {
  const bgRgb = extractRgb(style.bg)
  if (bgRgb) {
    cell.fill = {
      type: "pattern",
      pattern: "solid",
      fgColor: { argb: bgRgb.replace(/^#/, "") },
    }
  }
}

/** Apply horizontal alignment from Univer style. */
function applyAlignment(cell: ExcelJS.Cell, style: IStyleData): void {
  if (style.ht == null) return
  const map: Record<number, ExcelJS.Alignment["horizontal"]> = {
    0: "left",
    1: "center",
    2: "right",
  }
  cell.alignment = { horizontal: map[style.ht] ?? "left" }
}

/** Apply border edges from Univer IBorderData. */
function applyBorders(cell: ExcelJS.Cell, style: IStyleData): void {
  if (!style.bd) return
  const border: Partial<ExcelJS.Borders> = {}
  const edges: [keyof IBorderData, keyof ExcelJS.Borders][] = [
    ["t", "top"],
    ["b", "bottom"],
    ["l", "left"],
    ["r", "right"],
  ]
  for (const [key, excelKey] of edges) {
    const edge = style.bd[key] as IBorderStyleData | undefined
    if (!edge?.s) continue
    const borderStyle = toBorderStyle(edge.s)
    if (!borderStyle) continue
    const edgeRgb = extractRgb(edge.cl)
    border[excelKey] = {
      style: borderStyle,
      color: edgeRgb ? { argb: edgeRgb.replace(/^#/, "") } : { argb: "000000" },
    }
  }
  if (Object.keys(border).length > 0) cell.border = border
}

// ── Helpers ─────────────────────────────────────────────────────────

/** Check if a text decoration (underline/strikethrough) is active. */
function isDecorationActive(dec: ITextDecoration | null | undefined): boolean {
  if (!dec) return false
  return dec.s === BooleanNumber.TRUE
}

/** Extract RGB string from Univer IColorStyle. */
function extractRgb(color: Nullable<IColorStyle>): string | undefined {
  return color?.rgb ?? undefined
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
