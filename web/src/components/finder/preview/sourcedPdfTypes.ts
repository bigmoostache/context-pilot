/**
 * Types, color map, and ZIP-bundle parser for the `.sourced_pdf` format — a
 * self-contained audit bundle pairing a target PDF with extraction values and
 * cross-reference qualifications. See `.uploads/sourced_pdf/sourced_pdf/SPEC.md`.
 */
import JSZip from "jszip"
import { load as yamlLoad } from "js-yaml"

// ── domain types ──────────────────────────────────────────────────

export interface Metadata {
  documents: DocEntry[]
  target_doc_id: string
  refs_schema_path: string
}
export interface DocEntry {
  id: string
  path: string
  role: "sujet" | "source"
  sha256: string
}
export interface ValueLocator {
  page?: number
  bbox?: [number, number, number, number]
  sheet?: string
  cell?: string
}
export interface Value {
  id: string
  document_id: string
  text: string
  number: number | null
  locator: ValueLocator
}
export interface Arrow {
  value_id: string
  document_id: string
  sign?: "+" | "-"
}
export interface Ref {
  id: string
  type: string
  subject: { value_id: string; document_id: string }
  arrows: Arrow[]
  ecart?: number
  ecart_abs?: number
  concordance?: boolean
  status?: string
  comment?: string
}

/** Parsed, ready-to-render bundle. */
export interface SourcedBundle {
  metadata: Metadata
  targetDocBytes: Uint8Array
  /** Every document's raw bytes, keyed by document id. */
  docBytes: Map<string, Uint8Array>
  values: Value[]
  refs: Ref[]
  valuesById: Map<string, Value>
}

/** Infer document type from its path extension. */
export function docType(path: string): "pdf" | "xlsx" | "unknown" {
  const ext = path.split(".").pop()?.toLowerCase() ?? ""
  if (ext === "pdf") return "pdf"
  if (["xlsx", "xls", "xlsm", "xlsb", "ods"].includes(ext)) return "xlsx"
  return "unknown"
}

// ── ref-type color palette (from render.py) ───────────────────────

export const REF_COLORS: Record<string, string> = {
  report_bg: "#3B82F6",
  "report_n-1": "#A855F7",
  report_internal: "#14B8A6",
  arithmetic: "#F97316",
  unassigned: "#9CA3AF",
}

export const REF_LABELS: Record<string, string> = {
  report_bg: "Bilan / Grand-livre",
  "report_n-1": "Rapport N-1",
  report_internal: "Interne",
  arithmetic: "Arithmétique",
  unassigned: "Non qualifié",
}

export function refColor(type: string): string {
  return REF_COLORS[type] ?? "#9CA3AF"
}

export function refLabel(type: string): string {
  return REF_LABELS[type] ?? type
}

// ── ZIP bundle parser ─────────────────────────────────────────────

/** Find the common prefix inside the ZIP (handles single-folder wrappers). */
function findPrefix(files: string[]): string {
  const meta = files.find((f) => f.endsWith("metadata.yaml") && !f.startsWith("__MACOSX"))
  if (!meta) throw new Error("No metadata.yaml in bundle")
  return meta.replace("metadata.yaml", "")
}

async function readYaml<T>(zip: JSZip, path: string): Promise<T> {
  const file = zip.file(path)
  if (!file) throw new Error(`Missing ${path}`)
  const text = await file.async("text")
  return yamlLoad(text) as T
}

/** Parse a dot-separated value reference ("doc_0.value_34") used by the v1
 *  production format. Returns the full string as `value_id` (composite key). */
function parseValueRef(dotRef: string): { value_id: string; document_id: string } {
  const dot = dotRef.indexOf(".")
  if (dot < 0) return { value_id: dotRef, document_id: "" }
  return { document_id: dotRef.slice(0, dot), value_id: dotRef }
}

/**
 * Decompress and parse a `.sourced_pdf` ZIP bundle into a render-ready
 * {@link SourcedBundle}. Handles two metadata dialects:
 *
 * - **v0** (generated bundles): `target_doc_id`, `path` per doc, flat value
 *   arrays with `document_id` per value, `arrows` array in refs.
 * - **v1** (production/spdf-toolkit): `target`, `filename` per doc, per-file
 *   `doc` + nested `values` array with flat page/bbox, singular `arrow` or
 *   `operands` in refs, dot-separated value references.
 */
export async function parseSourcedBundle(zipBytes: ArrayBuffer): Promise<SourcedBundle> {
  const zip = await JSZip.loadAsync(zipBytes)
  const files = Object.keys(zip.files)
  const pfx = findPrefix(files)

  // ── metadata (normalize field names) ──
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const raw = await readYaml<any>(zip, pfx + "metadata.yaml")

  /** Resolve a document's path inside the ZIP — tries multiple naming
   *  conventions: explicit `path`, bare `filename`, `id__filename` (v1). */
  function resolveDocPath(d: { id: string; path?: string; filename?: string }): string {
    if (d.path) return d.path
    const fn = d.filename ?? d.id
    const candidates = [`documents/${fn}`, `documents/${d.id}__${fn}`]
    for (const c of candidates) { if (zip.file(pfx + c)) return c }
    // Last resort: scan documents/ for a file ending with the filename
    const suffix = `/${fn}`
    const match = files.find((f) => f.endsWith(suffix) && !f.includes("__MACOSX"))
    if (match) return match.slice(pfx.length)
    return candidates[0] // will fail at load time with a clear error
  }

  const metadata: Metadata = {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    documents: (raw.documents as any[]).map((d) => ({
      id: d.id,
      path: resolveDocPath(d),
      role: d.role,
      sha256: d.sha256,
    })),
    target_doc_id: raw.target_doc_id ?? raw.target,
    refs_schema_path: raw.refs_schema_path ?? "refs-schema.yaml",
  }
  const targetDoc = metadata.documents.find((d) => d.id === metadata.target_doc_id)
  if (!targetDoc) throw new Error(`Target doc ${metadata.target_doc_id} not in metadata`)

  // ── document bytes ──
  const pdfFile = zip.file(pfx + targetDoc.path)
  if (!pdfFile) throw new Error(`PDF not found at ${targetDoc.path}`)
  const targetDocBytes = await pdfFile.async("uint8array")
  const docBytes = new Map<string, Uint8Array>()
  for (const doc of metadata.documents) {
    const f = zip.file(pfx + doc.path)
    if (f) docBytes.set(doc.id, await f.async("uint8array"))
  }

  // ── values (two dialects) ──
  const values: Value[] = []
  const valPrefix = pfx + "values/"
  for (const path of files) {
    if (!path.startsWith(valPrefix) || !path.endsWith(".yaml")) continue
    if (path.includes("__MACOSX")) continue
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const body = await readYaml<any>(zip, path)
    if (body && typeof body === "object" && "doc" in body && Array.isArray(body.values)) {
      // v1 production format: file-level doc, flat page/bbox per value
      const docId = body.doc as string
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      for (const v of body.values as any[]) {
        values.push({
          id: `${docId}.${v.id}`,
          document_id: docId,
          text: v.text,
          number: v.number ?? null,
          locator: { page: v.page, bbox: v.bbox, sheet: v.sheet, cell: v.cell },
        })
      }
    } else if (Array.isArray(body)) {
      // v0 generated format: flat array with document_id per value
      values.push(...(body as Value[]))
    }
  }
  const valuesById = new Map(values.map((v) => [v.id, v]))

  // ── refs (two dialects) ──
  const refs: Ref[] = []
  const refPrefix = pfx + "refs/"
  for (const path of files) {
    if (!path.startsWith(refPrefix) || !path.endsWith(".yaml")) continue
    if (path.includes("__MACOSX")) continue
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const body = await readYaml<any[]>(zip, path)
    if (!Array.isArray(body)) continue
    for (const r of body) {
      const ref: Ref = {
        id: r.id, type: r.type,
        subject: typeof r.subject === "string" ? parseValueRef(r.subject) : r.subject,
        arrows: [],
        ecart: r.ecart, ecart_abs: r.ecart_abs,
        concordance: r.concordance, status: r.status, comment: r.comment,
      }
      if (r.arrows) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ref.arrows = (r.arrows as any[]).map((a) =>
          typeof a === "string" ? parseValueRef(a) : a)
      } else if (r.arrow) {
        ref.arrows = [parseValueRef(r.arrow as string)]
      } else if (r.operands) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ref.arrows = (r.operands as any[]).map((op) => ({
          ...parseValueRef(op.arrow as string),
          sign: op.sign === "+" ? ("+" as const) : ("-" as const),
        }))
      }
      refs.push(ref)
    }
  }

  return { metadata, targetDocBytes, docBytes, values, refs, valuesById }
}
