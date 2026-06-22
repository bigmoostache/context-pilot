/**
 * Integrated Finder preview for `.sourced_pdf` bundles — the true split-view
 * cross-reference auditor. Left: target PDF with subject highlights. Right:
 * the actual source document (PDF rendered via pdf.js, or Excel value card)
 * showing the arrow value when a ref is selected.
 *
 * Fetches raw ZIP bytes, decompresses with JSZip, renders pages via pdf.js
 * canvas, overlays clickable highlight divs from PyMuPDF bbox coordinates.
 */
import { useEffect, useMemo, useRef, useState } from "react"
import { ChevronLeft, ChevronRight, Check, X, FileSpreadsheet } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { rawUrl } from "@/lib/api"
import { cn } from "@/lib/utils"
import { PreviewStatus } from "./previewParts"
import {
  type DocEntry,
  type Ref,
  type SourcedBundle,
  type Value,
  docType,
  parseSourcedBundle,
  refColor,
  refLabel,
  REF_COLORS,
} from "./sourcedPdfTypes"

/* ── pdf.js bootstrap ──────────────────────────────────────────── */
import * as pdfjsLib from "pdfjs-dist"

pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url,
).href

/* ── constants ─────────────────────────────────────────────────── */
const PDF_SCALE = 1.5

/* ── main component ────────────────────────────────────────────── */

export function LiveSourcedPdfPreview({
  agentId,
  node,
}: {
  agentId: string
  node: FinderNode
}) {
  const [bundle, setBundle] = useState<SourcedBundle | null>(null)
  const [pdfDoc, setPdfDoc] = useState<pdfjsLib.PDFDocumentProxy | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [page, setPage] = useState(1)
  const [numPages, setNumPages] = useState(0)
  const [selectedRefId, setSelectedRefId] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const resp = await fetch(rawUrl(agentId, node.path))
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`)
        const bytes = await resp.arrayBuffer()
        const b = await parseSourcedBundle(bytes)
        if (cancelled) return
        setBundle(b)
        const doc = await pdfjsLib.getDocument({ data: b.targetDocBytes }).promise
        if (cancelled) return
        setPdfDoc(doc)
        setNumPages(doc.numPages)
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : "Load failed")
      }
    })()
    return () => { cancelled = true }
  }, [agentId, node.path])

  if (error) return <PreviewStatus label={`Error: ${error}`} />
  if (!bundle || !pdfDoc) return <PreviewStatus label="Loading sourced PDF…" />

  const selectedRef = selectedRefId
    ? bundle.refs.find((r) => r.id === selectedRefId) ?? null
    : null

  // Resolve which source doc the selected ref points to (first arrow's document)
  const arrowDoc = selectedRef?.arrows[0]
    ? bundle.metadata.documents.find((d) => d.id === selectedRef.arrows[0].document_id)
    : null

  return (
    <div className="flex h-full min-h-0 flex-col">
      <NavBar page={page} total={numPages} setPage={setPage} bundle={bundle} />
      {/* selected ref detail strip */}
      {selectedRef && (
        <RefStrip ref_={selectedRef} bundle={bundle} onClose={() => setSelectedRefId(null)} />
      )}
      <div className="flex min-h-0 flex-1">
        {/* LEFT: target PDF */}
        <div className="flex min-w-0 flex-1 flex-col border-r border-border">
          <PdfPage
            pdfDoc={pdfDoc}
            page={page}
            bundle={bundle}
            selectedRefId={selectedRefId}
            onSelectRef={(id, pg) => { setSelectedRefId(id); if (pg) setPage(pg) }}
          />
        </div>
        {/* RIGHT: source document */}
        <div className="flex min-w-0 flex-1 flex-col">
          <SourcePane bundle={bundle} selectedRef={selectedRef} arrowDoc={arrowDoc} />
        </div>
      </div>
    </div>
  )
}

/* ── nav bar ───────────────────────────────────────────────────── */

function NavBar({
  page, total, setPage, bundle,
}: {
  page: number; total: number; setPage: (p: number) => void; bundle: SourcedBundle
}) {
  const concordant = bundle.refs.filter((r) => r.concordance).length
  const pct = bundle.refs.length > 0 ? Math.round((concordant / bundle.refs.length) * 100) : 0
  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-border bg-muted/40 px-3 py-1.5">
      <div className="flex items-center gap-1">
        <button onClick={() => setPage(Math.max(1, page - 1))} disabled={page <= 1}
          className="rounded p-0.5 text-muted-foreground hover:bg-muted disabled:opacity-30">
          <ChevronLeft className="size-4" />
        </button>
        <span className="min-w-[4ch] text-center text-[11px] tabular-nums text-foreground/80">
          {page} / {total}
        </span>
        <button onClick={() => setPage(Math.min(total, page + 1))} disabled={page >= total}
          className="rounded p-0.5 text-muted-foreground hover:bg-muted disabled:opacity-30">
          <ChevronRight className="size-4" />
        </button>
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        {Object.entries(REF_COLORS).map(([type, color]) => {
          const count = bundle.refs.filter((r) => r.type === type).length
          if (count === 0) return null
          return (
            <span key={type}
              className="flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium"
              style={{ backgroundColor: color + "20", color }}>
              <span className="size-1.5 rounded-full" style={{ backgroundColor: color }} />
              {refLabel(type)} ({count})
            </span>
          )
        })}
      </div>
      <span className={cn("ml-auto text-[10.5px] font-medium",
        pct >= 95 ? "text-[var(--ok)]" : "text-[var(--warn)]")}>
        {pct}% concordant
      </span>
    </div>
  )
}

/* ── selected ref detail strip ─────────────────────────────────── */

function RefStrip({
  ref_, bundle, onClose,
}: {
  ref_: Ref; bundle: SourcedBundle; onClose: () => void
}) {
  const subVal = bundle.valuesById.get(ref_.subject.value_id)
  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-border px-3 py-1"
      style={{ backgroundColor: refColor(ref_.type) + "08" }}>
      <span className="size-2 rounded-full" style={{ backgroundColor: refColor(ref_.type) }} />
      <span className="text-[11px] font-medium" style={{ color: refColor(ref_.type) }}>
        {refLabel(ref_.type)}
      </span>
      <span className="text-[11px] text-foreground/70 truncate">
        {subVal?.text ?? ref_.subject.value_id}
      </span>
      {ref_.concordance != null && (ref_.concordance
        ? <Check className="size-3.5 text-[var(--ok)]" />
        : <X className="size-3.5 text-[var(--danger)]" />
      )}
      {ref_.ecart != null && (
        <span className={cn("text-[10.5px] tabular-nums",
          !ref_.concordance && "font-medium text-[var(--danger)]")}>
          écart {fmt(ref_.ecart)}
        </span>
      )}
      {ref_.arrows.map((a, i) => {
        const av = bundle.valuesById.get(a.value_id)
        return (
          <span key={i} className="text-[10.5px] text-muted-foreground truncate">
            → {av?.text ?? a.value_id}
            {av?.locator.cell && ` (${av.locator.sheet}!${av.locator.cell})`}
          </span>
        )
      })}
      <button onClick={onClose} className="ml-auto rounded p-0.5 hover:bg-muted">
        <X className="size-3.5 text-muted-foreground" />
      </button>
    </div>
  )
}

/* ── PDF page with highlight overlays (shared) ─────────────────── */

function PdfPage({
  pdfDoc, page, bundle, selectedRefId, onSelectRef, highlightValueId,
}: {
  pdfDoc: pdfjsLib.PDFDocumentProxy
  page: number
  bundle: SourcedBundle
  selectedRefId: string | null
  onSelectRef: (id: string | null, page?: number) => void
  /** If set, only highlight this single value (for source side). */
  highlightValueId?: string
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [vp, setVp] = useState<{ width: number; height: number } | null>(null)

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      const pg = await pdfDoc.getPage(page)
      const viewport = pg.getViewport({ scale: PDF_SCALE })
      if (cancelled) return
      setVp({ width: viewport.width, height: viewport.height })
      const canvas = canvasRef.current
      if (!canvas) return
      canvas.width = viewport.width
      canvas.height = viewport.height
      const ctx = canvas.getContext("2d")
      if (!ctx) return
      await pg.render({ canvasContext: ctx, viewport }).promise
    })()
    return () => { cancelled = true }
  }, [pdfDoc, page])

  const highlights = useMemo(() => {
    if (highlightValueId) {
      // Source side: highlight only the specific arrow value
      const v = bundle.valuesById.get(highlightValueId)
      if (!v || v.locator.page !== page || !v.locator.bbox) return []
      // Find the ref for coloring
      const ref = bundle.refs.find((r) =>
        r.arrows.some((a) => a.value_id === highlightValueId))
      return ref ? [{ value: v, ref }] : []
    }
    // Target side: highlight all subject values on this page
    const targetId = bundle.metadata.target_doc_id
    const subjectMap = new Map<string, Ref>()
    for (const ref of bundle.refs) {
      if (ref.subject.document_id === targetId)
        subjectMap.set(ref.subject.value_id, ref)
    }
    const result: { value: Value; ref: Ref }[] = []
    for (const v of bundle.values) {
      if (v.document_id !== targetId || v.locator.page !== page) continue
      const ref = subjectMap.get(v.id)
      if (ref && v.locator.bbox) result.push({ value: v, ref })
    }
    return result
  }, [bundle, page, highlightValueId])

  if (!vp) return <PreviewStatus label="Rendering…" />

  return (
    <div className="relative min-h-0 flex-1 overflow-auto bg-neutral-100 dark:bg-neutral-900">
      <div className="flex items-start justify-center p-4">
        <div className="relative" style={{ width: vp.width, height: vp.height }}>
          <canvas ref={canvasRef} className="block rounded shadow-lg" />
          {highlights.map(({ value, ref }) => {
            const [x0, y0, x1, y1] = value.locator.bbox!
            const s = PDF_SCALE
            const color = refColor(ref.type)
            const selected = ref.id === selectedRefId
            return (
              <div key={value.id}
                onClick={() => onSelectRef(selected ? null : ref.id, value.locator.page)}
                title={`${value.text} — ${refLabel(ref.type)}`}
                className={cn("absolute cursor-pointer rounded-sm border-2 transition-all",
                  selected ? "ring-2 ring-offset-1 z-10" : "hover:brightness-110")}
                style={{
                  left: x0 * s, top: y0 * s,
                  width: (x1 - x0) * s, height: (y1 - y0) * s,
                  backgroundColor: color + "30", borderColor: color, ringColor: color,
                }}
              />
            )
          })}
        </div>
      </div>
    </div>
  )
}

/* ── source document pane (right side) ─────────────────────────── */

function SourcePane({
  bundle, selectedRef, arrowDoc,
}: {
  bundle: SourcedBundle; selectedRef: Ref | null; arrowDoc: DocEntry | null | undefined
}) {
  if (!selectedRef || !arrowDoc) {
    return (
      <div className="flex flex-1 items-center justify-center text-[12px] text-muted-foreground/50">
        <p className="text-center">
          Cliquez sur un highlight<br/>pour afficher le document source
        </p>
      </div>
    )
  }

  const sourceBytes = bundle.docBytes.get(arrowDoc.id)
  if (!sourceBytes) {
    return <PreviewStatus label={`Source document "${arrowDoc.path}" not found in bundle`} />
  }

  const type = docType(arrowDoc.path)
  const arrowValue = bundle.valuesById.get(selectedRef.arrows[0].value_id)

  if (type === "pdf") {
    return (
      <SourcePdfViewer
        docBytes={sourceBytes}
        bundle={bundle}
        selectedRef={selectedRef}
        arrowValue={arrowValue}
      />
    )
  }

  // Excel / unknown → value info card
  return (
    <ExcelSourceCard
      doc={arrowDoc}
      ref_={selectedRef}
      bundle={bundle}
    />
  )
}

/* ── source PDF viewer ─────────────────────────────────────────── */

function SourcePdfViewer({
  docBytes, bundle, selectedRef, arrowValue,
}: {
  docBytes: Uint8Array
  bundle: SourcedBundle
  selectedRef: Ref
  arrowValue: Value | undefined
}) {
  const [doc, setDoc] = useState<pdfjsLib.PDFDocumentProxy | null>(null)
  const [page, setPage] = useState(1)
  const [numPages, setNumPages] = useState(0)
  const [err, setErr] = useState(false)
  const bytesRef = useRef(docBytes)

  useEffect(() => {
    // Reload only when source doc bytes change
    if (bytesRef.current === docBytes && doc) return
    bytesRef.current = docBytes
    let cancelled = false
    pdfjsLib.getDocument({ data: docBytes }).promise.then((d) => {
      if (cancelled) return
      setDoc(d)
      setNumPages(d.numPages)
      setErr(false)
    }).catch(() => { if (!cancelled) setErr(true) })
    return () => { cancelled = true }
  }, [docBytes, doc])

  // Navigate to the arrow value's page when ref changes
  useEffect(() => {
    if (arrowValue?.locator.page) setPage(arrowValue.locator.page)
  }, [arrowValue])

  if (err) return <PreviewStatus label="Failed to load source PDF" />
  if (!doc) return <PreviewStatus label="Loading source PDF…" />

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-muted/30 px-3 py-1">
        <button onClick={() => setPage(Math.max(1, page - 1))} disabled={page <= 1}
          className="rounded p-0.5 text-muted-foreground hover:bg-muted disabled:opacity-30">
          <ChevronLeft className="size-3.5" />
        </button>
        <span className="text-[10.5px] tabular-nums text-foreground/60">{page}/{numPages}</span>
        <button onClick={() => setPage(Math.min(numPages, page + 1))} disabled={page >= numPages}
          className="rounded p-0.5 text-muted-foreground hover:bg-muted disabled:opacity-30">
          <ChevronRight className="size-3.5" />
        </button>
        <span className="ml-1 truncate text-[10px] text-muted-foreground/50">Source PDF</span>
      </div>
      <PdfPage
        pdfDoc={doc}
        page={page}
        bundle={bundle}
        selectedRefId={selectedRef.id}
        onSelectRef={() => {}}
        highlightValueId={selectedRef.arrows[0]?.value_id}
      />
    </div>
  )
}

/* ── Excel source card ─────────────────────────────────────────── */

function ExcelSourceCard({
  doc, ref_, bundle,
}: {
  doc: DocEntry; ref_: Ref; bundle: SourcedBundle
}) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-4 p-6">
      <div className="rounded-xl border border-border bg-card p-6 shadow-sm max-w-[340px] w-full">
        <div className="flex items-center gap-2 mb-4">
          <FileSpreadsheet className="size-5 text-[var(--ok)]" />
          <span className="text-[12px] font-semibold text-foreground/80">
            {doc.path.split("/").pop()}
          </span>
        </div>
        <div className="space-y-2">
          {ref_.arrows.map((arrow, i) => {
            const val = bundle.valuesById.get(arrow.value_id)
            if (!val) return null
            return (
              <div key={i} className="rounded-lg border border-border/50 bg-muted/30 px-3 py-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-[11px] font-medium text-foreground/70">{val.text}</span>
                  {val.number != null && (
                    <span className="text-[11px] tabular-nums font-semibold text-foreground/80">
                      {fmt(val.number)}
                    </span>
                  )}
                </div>
                {(val.locator.sheet || val.locator.cell) && (
                  <p className="mt-1 text-[10px] text-muted-foreground/60">
                    {val.locator.sheet && <span className="font-medium">{val.locator.sheet}</span>}
                    {val.locator.cell && <span> · cellule {val.locator.cell}</span>}
                  </p>
                )}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

/* ── tiny helpers ──────────────────────────────────────────────── */

function fmt(n: number): string {
  return n.toLocaleString("fr-FR", { minimumFractionDigits: 0, maximumFractionDigits: 2 })
}
