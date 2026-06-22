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
  values: Value[]
  refs: Ref[]
  valuesById: Map<string, Value>
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

/**
 * Decompress and parse a `.sourced_pdf` ZIP bundle into a render-ready
 * {@link SourcedBundle}. Resolves the target PDF bytes, all extracted values,
 * and all cross-reference qualifications.
 */
export async function parseSourcedBundle(zipBytes: ArrayBuffer): Promise<SourcedBundle> {
  const zip = await JSZip.loadAsync(zipBytes)
  const files = Object.keys(zip.files)
  const pfx = findPrefix(files)

  // metadata
  const metadata = await readYaml<Metadata>(zip, pfx + "metadata.yaml")
  const targetDoc = metadata.documents.find((d) => d.id === metadata.target_doc_id)
  if (!targetDoc) throw new Error(`Target doc ${metadata.target_doc_id} not in metadata`)

  // target PDF bytes
  const pdfFile = zip.file(pfx + targetDoc.path)
  if (!pdfFile) throw new Error(`PDF not found at ${targetDoc.path}`)
  const targetDocBytes = await pdfFile.async("uint8array")

  // values — one YAML per document under values/
  const values: Value[] = []
  const valPrefix = pfx + "values/"
  for (const path of files) {
    if (!path.startsWith(valPrefix) || !path.endsWith(".yaml")) continue
    if (path.includes("__MACOSX")) continue
    const docVals = await readYaml<Value[]>(zip, path)
    if (Array.isArray(docVals)) values.push(...docVals)
  }
  const valuesById = new Map(values.map((v) => [v.id, v]))

  // refs — one or more YAML files under refs/
  const refs: Ref[] = []
  const refPrefix = pfx + "refs/"
  for (const path of files) {
    if (!path.startsWith(refPrefix) || !path.endsWith(".yaml")) continue
    if (path.includes("__MACOSX")) continue
    const docRefs = await readYaml<Ref[]>(zip, path)
    if (Array.isArray(docRefs)) refs.push(...docRefs)
  }

  return { metadata, targetDocBytes, values, refs, valuesById }
}
