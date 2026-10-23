import type { FilterOp } from "@/lib/api/generated/types.gen"

import { numProp, str } from "../openuiKit"
import type { OpenUiNode, OpenUiValue } from "../openuiParse"

// ── SqlTable — configuration model + cell formatting ─────────────────────────
//
// Every knob is a scalar openui prop, so the LLM configures a table in one line:
//
//   SqlTable("SELECT …", page_size: 25, sort: "score desc", hide: "id",
//            pin: "name", format: "score=percent, cost=currency:EUR",
//            labels: "n=Count", density: compact, selectable: true,
//            send: "Investigate rows {json}")
//
// Map-style props (`format`, `labels`, `align`, `widths`) are `col=value` pairs
// separated by commas; list props (`hide`, `pin`, `page_sizes`) are comma lists.

/** Row height / padding preset. */
export type Density = "compact" | "normal" | "comfortable"

/** How a column's cells render. */
export type Format =
  | "text"
  | "number"
  | "integer"
  | "percent"
  | "currency"
  | "bytes"
  | "duration"
  | "date"
  | "datetime"
  | "bool"
  | "badge"
  | "code"
  | "bar"
  | "link"

/** A column format plus its optional argument (`currency:EUR`, `number:2`). */
export interface ColumnFormat {
  kind: Format
  arg: string | undefined
}

/** Static table configuration, parsed once from the node's props. */
export interface TableConfig {
  sql: string
  title: string
  pageSize: number
  pageSizes: number[]
  density: Density
  striped: boolean
  selectable: boolean
  send: string | undefined
  sendLabel: string
  toolbar: boolean
  search: boolean
  filters: boolean
  exportCsv: boolean
  rowNumbers: boolean
  wrap: boolean
  maxHeight: number
  hidden: string[]
  pinned: string[]
  sort: { column: string; desc: boolean }[]
  formats: Record<string, ColumnFormat>
  labels: Record<string, string>
  align: Record<string, "left" | "center" | "right">
  widths: Record<string, number>
}

/** Every filter operator the backend implements, with its menu label. */
export const OPS: { op: FilterOp; label: string; unary?: true }[] = [
  { op: "contains", label: "contains" },
  { op: "not_contains", label: "does not contain" },
  { op: "starts_with", label: "starts with" },
  { op: "ends_with", label: "ends with" },
  { op: "eq", label: "=" },
  { op: "neq", label: "≠" },
  { op: "gt", label: ">" },
  { op: "gte", label: "≥" },
  { op: "lt", label: "<" },
  { op: "lte", label: "≤" },
  { op: "is_null", label: "is empty", unary: true },
  { op: "not_null", label: "is not empty", unary: true },
]

const FORMATS = new Set<string>([
  "text",
  "number",
  "integer",
  "percent",
  "currency",
  "bytes",
  "duration",
  "date",
  "datetime",
  "bool",
  "badge",
  "code",
  "bar",
  "link",
])

const DENSITIES = new Set<string>(["compact", "normal", "comfortable"])

const ALIGNS = new Set<string>(["left", "center", "right"])

/** Split a comma list, trimming and dropping blanks. */
export const list = (v: OpenUiValue | undefined): string[] =>
  typeof v === "string"
    ? v
        .split(",")
        .map((x) => x.trim())
        .filter((x) => x.length > 0)
    : []

/** Parse `a=x, b=y` into a record. */
function pairs(v: OpenUiValue | undefined): Record<string, string> {
  const out: Record<string, string> = {}
  for (const item of list(v)) {
    const eq = item.indexOf("=")
    if (eq > 0) out[item.slice(0, eq).trim()] = item.slice(eq + 1).trim()
  }
  return out
}

const flag = (v: OpenUiValue | undefined, fallback: boolean): boolean =>
  typeof v === "boolean" ? v : v === "true" ? true : v === "false" ? false : fallback

function parseFormats(v: OpenUiValue | undefined): Record<string, ColumnFormat> {
  const out: Record<string, ColumnFormat> = {}
  const entries = Object.entries(pairs(v))
  for (const [col, spec] of entries) {
    const [kind = "", arg] = spec.split(":")
    if (FORMATS.has(kind)) out[col] = { kind: kind as Format, arg }
  }
  return out
}

function parseAlign(v: OpenUiValue | undefined): TableConfig["align"] {
  const out: TableConfig["align"] = {}
  const entries = Object.entries(pairs(v))
  for (const [col, a] of entries) {
    if (ALIGNS.has(a)) out[col] = a as "left" | "center" | "right"
  }
  return out
}

function parseWidths(v: OpenUiValue | undefined): Record<string, number> {
  const out: Record<string, number> = {}
  const entries = Object.entries(pairs(v))
  for (const [col, w] of entries) {
    const n = Number(w)
    if (Number.isFinite(n) && n > 24) out[col] = n
  }
  return out
}

/** `"score desc, name"` → sort keys. */
const parseSort = (v: OpenUiValue | undefined): TableConfig["sort"] =>
  list(v).map((k) => {
    const [column = "", dir] = k.split(/\s+/)
    return { column, desc: dir?.toLowerCase() === "desc" }
  })

/** Read every SqlTable prop, with defaults. */
export function parseConfig(node: OpenUiNode, sql: string): TableConfig {
  const p = node.props
  const pageSize = Math.min(200, Math.max(1, numProp(node, "page_size") ?? 25))
  const sizes = list(p["page_sizes"])
    .map(Number)
    .filter((n) => Number.isSafeInteger(n) && n > 0 && n <= 200)
  const density = str(p["density"], "normal")
  const send = typeof p["send"] === "string" ? p["send"] : undefined
  return {
    sql,
    title: str(p["title"]),
    pageSize,
    pageSizes: [...new Set([...(sizes.length > 0 ? sizes : [10, 25, 50, 100]), pageSize])].toSorted(
      (a, b) => a - b,
    ),
    density: DENSITIES.has(density) ? (density as Density) : "normal",
    striped: flag(p["striped"], false),
    selectable: flag(p["selectable"], send !== undefined),
    send,
    sendLabel: str(p["send_label"], "Send to agent"),
    toolbar: flag(p["toolbar"], true),
    search: flag(p["search"], true),
    filters: flag(p["filters"], true),
    exportCsv: flag(p["export"], true),
    rowNumbers: flag(p["row_numbers"], false),
    wrap: flag(p["wrap"], false),
    maxHeight: numProp(node, "max_height") ?? 448,
    hidden: list(p["hide"]),
    pinned: list(p["pin"]),
    sort: parseSort(p["sort"]),
    formats: parseFormats(p["format"]),
    labels: pairs(p["labels"]),
    align: parseAlign(p["align"]),
    widths: parseWidths(p["widths"]),
  }
}

// ── cell formatting ──────────────────────────────────────────────────────────

const num = (v: string): number | undefined => {
  const n = Number(v)
  return v.trim() !== "" && Number.isFinite(n) ? n : undefined
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"]

function bytes(n: number): string {
  let v = n
  let i = 0
  while (Math.abs(v) >= 1024 && i < BYTE_UNITS.length - 1) {
    v /= 1024
    i += 1
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${BYTE_UNITS[i] ?? "B"}`
}

function duration(secs: number): string {
  if (secs < 1) return `${String(Math.round(secs * 1000))} ms`
  if (secs < 60) return `${secs.toFixed(secs < 10 ? 1 : 0)} s`
  const m = Math.floor(secs / 60)
  if (m < 60) return `${String(m)}m ${String(Math.round(secs % 60))}s`
  return `${String(Math.floor(m / 60))}h ${String(m % 60)}m`
}

/** A date from epoch seconds/ms or an ISO string; `undefined` when unparsable. */
function toDate(v: string): Date | undefined {
  const n = num(v)
  const d = n === undefined ? new Date(v) : new Date(n < 1e11 ? n * 1000 : n)
  return Number.isNaN(d.getTime()) ? undefined : d
}

type Formatter = (v: string, n: number | undefined, arg: string | undefined) => string

/** Fraction digits from a format argument (`number:2`), if given. */
const digitsOf = (arg: string | undefined): number | undefined =>
  arg === undefined ? undefined : Number(arg)

/** Numeric formatters fall back to the raw text when the cell is not a number. */
const numeric_ =
  (f: (n: number, arg: string | undefined) => string): Formatter =>
  (v, n, arg) =>
    n === undefined ? v : f(n, arg)

const FORMATTERS: Partial<Record<Format, Formatter>> = {
  number: numeric_((n, arg) => {
    const d = digitsOf(arg)
    return n.toLocaleString(
      undefined,
      d === undefined ? {} : { minimumFractionDigits: d, maximumFractionDigits: d },
    )
  }),
  integer: numeric_((n) => Math.round(n).toLocaleString()),
  // ≤ 1 reads as a ratio (0.42 → 42%), otherwise as an already-scaled percent.
  percent: numeric_((n, arg) => `${(Math.abs(n) <= 1 ? n * 100 : n).toFixed(digitsOf(arg) ?? 1)}%`),
  currency: numeric_((n, arg) =>
    n.toLocaleString(undefined, { style: "currency", currency: arg ?? "USD" }),
  ),
  bytes: numeric_((n) => bytes(n)),
  duration: numeric_((n) => duration(n)),
  date: (v) => toDate(v)?.toLocaleDateString() ?? v,
  datetime: (v) => toDate(v)?.toLocaleString() ?? v,
  bool: (v) => (truthy(v) ? "✓" : "✗"),
}

/** Display text for a non-null cell under `fmt` (badge/bar/bool/link styling is the grid's job). */
export function formatText(v: string, fmt: ColumnFormat | undefined): string {
  const f = fmt === undefined ? undefined : FORMATTERS[fmt.kind]
  return f === undefined ? v : f(v, num(v), fmt?.arg)
}

/** SQLite-ish truthiness for a bool column. */
export const truthy = (v: string): boolean =>
  !["0", "false", "no", "", "f"].includes(v.trim().toLowerCase())

/** Numeric value of a cell, for bar widths and right-alignment. */
export const numeric = num

const BADGE_TONES = ["signal", "ok", "warn", "danger", "interactive"]

/** A stable tone per distinct badge value, so equal values share a colour. */
export function badgeTone(v: string): string {
  let h = 0
  for (const c of v) h = (h * 31 + (c.codePointAt(0) ?? 0)) >>> 0
  return BADGE_TONES[h % BADGE_TONES.length] ?? "signal"
}

/** RFC-4180 CSV of `rows` under `columns`. */
export function toCsv(columns: string[], rows: (string | null)[][]): string {
  return [csvLine(columns), ...rows.map((r) => csvLine(r))].join("\n")
}

/** One CSV record. */
const csvLine = (r: (string | null)[]): string => r.map((v) => csvEscape(v)).join(",")

/** One CSV field, quoted when it holds a quote, comma or newline. */
function csvEscape(v: string | null): string {
  if (v === null) return ""
  return /[",\n]/.test(v) ? `"${v.replaceAll('"', '""')}"` : v
}

/** The page's rows as display cells (`null` = SQL NULL). */
export const cells = (rows: unknown[][]): (string | null)[][] =>
  rows.map((r) =>
    r.map((v) =>
      typeof v === "string" ? v : v === null || v === undefined ? null : JSON.stringify(v),
    ),
  )

/** Rows as JSON objects keyed by column (the `{json}` send placeholder). */
export const toObjects = (
  columns: string[],
  rows: (string | null)[][],
): Record<string, string | null>[] =>
  rows.map((r) => Object.fromEntries(columns.map((c, i) => [c, r[i] ?? null])))
