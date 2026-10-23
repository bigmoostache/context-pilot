import { strToU8, zipSync } from "fflate"

// ── SqlTable — minimal .xlsx writer (one sheet, header + rows) ────────────────
//
// An .xlsx is a zip of a few XML parts. Text cells are inline strings (no shared
// string table), numeric-looking cells are written as numbers so Excel can sum
// and sort them, and NULL cells are left empty.
//
// OOXML namespace URIs are identifiers, not fetched URLs, and must keep their
// exact `http:` spelling; they are assembled from short pieces below.

const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'

/** `http:` + `//schemas.openxmlformats.org` — the OOXML namespace root. */
const NS_ROOT = ["http:", "", "schemas.openxmlformats.org"].join("/")
const NS_PKG = `${NS_ROOT}/package/2006`
const NS_DOC = `${NS_ROOT}/officeDocument/2006/relationships`
const NS_MAIN = `${NS_ROOT}/spreadsheetml/2006/main`

/** `application/vnd.openxmlformats-…` content types, joined from short parts. */
const mime = (...parts: string[]): string =>
  ["application/vnd", "openxmlformats-officedocument", ...parts].join(".")
const SHEET_MIME = mime("spreadsheetml", "sheet")

/** One XML element: `<name a="v">body</name>`, or self-closing without a body. */
function tag(name: string, attrs: Record<string, string>, body?: string): string {
  const a = Object.entries(attrs)
    .map(([k, v]) => ` ${k}="${v}"`)
    .join("")
  return body === undefined ? `<${name}${a}/>` : `<${name}${a}>${body}</${name}>`
}

/** A relationships part with one relationship `rId1` of `type` → `target`. */
const rels = (type: string, target: string): string =>
  XML +
  tag(
    "Relationships",
    { xmlns: `${NS_PKG}/relationships` },
    tag("Relationship", { Id: "rId1", Type: `${NS_DOC}/${type}`, Target: target }),
  )

const CONTENT_TYPES =
  XML +
  tag(
    "Types",
    { xmlns: `${NS_PKG}/content-types` },
    [
      tag("Default", {
        Extension: "rels",
        ContentType: "application/vnd.openxmlformats-package.relationships+xml",
      }),
      tag("Default", { Extension: "xml", ContentType: "application/xml" }),
      tag("Override", { PartName: "/xl/workbook.xml", ContentType: `${SHEET_MIME}.main+xml` }),
      tag("Override", {
        PartName: "/xl/worksheets/sheet1.xml",
        ContentType: mime("spreadsheetml", "worksheet+xml"),
      }),
    ].join(""),
  )

/** True for XML-legal characters (tab, newline, CR, and everything from space up). */
const xmlLegal = (c: string): boolean => {
  const n = c.codePointAt(0) ?? 0
  return n >= 32 || n === 9 || n === 10 || n === 13
}

/** `s` without the control characters XML forbids. */
function xmlChars(s: string): string {
  let out = ""
  for (const c of s) if (xmlLegal(c)) out += c
  return out
}

/** XML-escape text and drop control characters Excel rejects. */
const esc = (s: string): string =>
  xmlChars(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")

/** Excel column letters for a 0-based index (0 → A, 26 → AA). */
function colName(i: number): string {
  let n = i + 1
  let out = ""
  while (n > 0) {
    const r = (n - 1) % 26
    out = String.fromCodePoint(65 + r) + out
    n = Math.floor((n - 1) / 26)
  }
  return out
}

/** Written as a number only when it round-trips exactly (keeps ids like "007" as text). */
const isPlainNumber = (v: string): boolean => {
  const n = Number(v)
  return v.trim() !== "" && Number.isFinite(n) && String(n) === v
}

function cell(v: string | null, ref: string): string {
  if (v === null) return ""
  if (isPlainNumber(v)) return tag("c", { r: ref }, `<v>${v}</v>`)
  return tag("c", { r: ref, t: "inlineStr" }, `<is><t xml:space="preserve">${esc(v)}</t></is>`)
}

/** An .xlsx workbook with one sheet named `sheet`: `columns` header, then `rows`. */
export function toXlsx(sheet: string, columns: string[], rows: (string | null)[][]): Blob {
  const lines = [columns, ...rows].map((r, y) =>
    tag(
      "row",
      { r: String(y + 1) },
      r.map((v, x) => cell(v, `${colName(x)}${String(y + 1)}`)).join(""),
    ),
  )
  const name = esc(sheet.replaceAll(/[\\/?*[\]:]/g, " ").slice(0, 31) || "Sheet1")
  const workbook = tag(
    "workbook",
    { xmlns: NS_MAIN, "xmlns:r": NS_DOC },
    tag("sheets", {}, tag("sheet", { name, sheetId: "1", "r:id": "rId1" })),
  )
  // Header row frozen at the top.
  const pane = tag("pane", {
    ySplit: "1",
    topLeftCell: "A2",
    activePane: "bottomLeft",
    state: "frozen",
  })
  const worksheet = tag(
    "worksheet",
    { xmlns: NS_MAIN },
    tag("sheetViews", {}, tag("sheetView", { workbookViewId: "0" }, pane)) +
      tag("sheetData", {}, lines.join("")),
  )
  const parts: Record<string, Uint8Array> = {
    "[Content_Types].xml": strToU8(CONTENT_TYPES),
    "_rels/.rels": strToU8(rels("officeDocument", "xl/workbook.xml")),
    "xl/_rels/workbook.xml.rels": strToU8(rels("worksheet", "worksheets/sheet1.xml")),
    "xl/workbook.xml": strToU8(XML + workbook),
    "xl/worksheets/sheet1.xml": strToU8(XML + worksheet),
  }
  return new Blob([zipSync(parts)], { type: SHEET_MIME })
}
