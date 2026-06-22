/**
 * Integrated Finder preview for `.sourced_pdf` bundles — the split-view
 * cross-reference auditor. Left: PDF pages with value highlights colored by ref
 * type. Right: inspector panel listing refs with concordance details.
 *
 * Fetches raw ZIP bytes from the backend's inline-serve endpoint, decompresses
 * with JSZip, renders pages via pdf.js canvas, and overlays clickable highlight
 * divs positioned from PyMuPDF bbox coordinates.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { ChevronLeft, ChevronRight, Check, X, ArrowRight } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { rawUrl } from "@/lib/api"
import { cn } from "@/lib/utils"
import { PreviewStatus } from "./previewParts"
import {
  type Ref,
  type SourcedBundle,
  type Value,
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

  // Load bundle
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

  return (
    <div className="flex h-full min-h-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <PageNav page={page} total={numPages} setPage={setPage} bundle={bundle} />
        <PdfPage
          pdfDoc={pdfDoc}
          page={page}
          bundle={bundle}
          selectedRefId={selectedRefId}
          onSelectRef={setSelectedRefId}
        />
      </div>
      <div className="w-[300px] shrink-0 overflow-y-auto border-l border-border bg-card">
        <RefPanel
          bundle={bundle}
          selectedRefId={selectedRefId}
          onSelectRef={(id, pg) => {
            setSelectedRefId(id)
            if (pg) setPage(pg)
          }}
        />
      </div>
    </div>
  )
}

/* ── page nav bar ──────────────────────────────────────────────── */

function PageNav({
  page,
  total,
  setPage,
  bundle,
}: {
  page: number
  total: number
  setPage: (p: number) => void
  bundle: SourcedBundle
}) {
  const concordant = bundle.refs.filter((r) => r.concordance).length
  const pct = bundle.refs.length > 0 ? Math.round((concordant / bundle.refs.length) * 100) : 0
  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-border bg-muted/40 px-3 py-1.5">
      <div className="flex items-center gap-1">
        <button
          onClick={() => setPage(Math.max(1, page - 1))}
          disabled={page <= 1}
          className="rounded p-0.5 text-muted-foreground hover:bg-muted disabled:opacity-30"
        >
          <ChevronLeft className="size-4" />
        </button>
        <span className="min-w-[4ch] text-center text-[11px] tabular-nums text-foreground/80">
          {page} / {total}
        </span>
        <button
          onClick={() => setPage(Math.min(total, page + 1))}
          disabled={page >= total}
          className="rounded p-0.5 text-muted-foreground hover:bg-muted disabled:opacity-30"
        >
          <ChevronRight className="size-4" />
        </button>
      </div>
      <div className="ml-auto flex items-center gap-2 text-[10.5px] text-muted-foreground">
        <span>{bundle.refs.length} refs</span>
        <span className={cn("font-medium", pct >= 95 ? "text-[var(--ok)]" : "text-[var(--warn)]")}>
          {pct}% concordant
        </span>
      </div>
    </div>
  )
}

/* ── PDF page renderer + highlight overlays ────────────────────── */

function PdfPage({
  pdfDoc,
  page,
  bundle,
  selectedRefId,
  onSelectRef,
}: {
  pdfDoc: pdfjsLib.PDFDocumentProxy
  page: number
  bundle: SourcedBundle
  selectedRefId: string | null
  onSelectRef: (id: string | null) => void
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

  // Values on this page that belong to subject positions of refs
  const highlights = useMemo(() => {
    const targetId = bundle.metadata.target_doc_id
    const subjectValueIds = new Map<string, Ref>()
    for (const ref of bundle.refs) {
      if (ref.subject.document_id === targetId) {
        subjectValueIds.set(ref.subject.value_id, ref)
      }
    }
    const result: { value: Value; ref: Ref }[] = []
    for (const v of bundle.values) {
      if (v.document_id !== targetId) continue
      if (v.locator.page !== page) continue
      const ref = subjectValueIds.get(v.id)
      if (ref && v.locator.bbox) result.push({ value: v, ref })
    }
    return result
  }, [bundle, page])

  if (!vp) return <PreviewStatus label="Rendering page…" />

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
              <div
                key={value.id}
                onClick={() => onSelectRef(selected ? null : ref.id)}
                title={`${value.text} — ${refLabel(ref.type)}`}
                className={cn(
                  "absolute cursor-pointer rounded-sm border-2 transition-all",
                  selected ? "ring-2 ring-offset-1 z-10" : "hover:brightness-110",
                )}
                style={{
                  left: x0 * s,
                  top: y0 * s,
                  width: (x1 - x0) * s,
                  height: (y1 - y0) * s,
                  backgroundColor: color + "30",
                  borderColor: color,
                  ringColor: color,
                }}
              />
            )
          })}
        </div>
      </div>
    </div>
  )
}

/* ── right panel: ref inspector ────────────────────────────────── */

function RefPanel({
  bundle,
  selectedRefId,
  onSelectRef,
}: {
  bundle: SourcedBundle
  selectedRefId: string | null
  onSelectRef: (id: string, page?: number) => void
}) {
  const scrollRef = useRef<HTMLDivElement>(null)

  // Group refs by type
  const groups = useMemo(() => {
    const m = new Map<string, Ref[]>()
    for (const ref of bundle.refs) {
      const arr = m.get(ref.type) ?? []
      arr.push(ref)
      m.set(ref.type, arr)
    }
    return m
  }, [bundle.refs])

  // Scroll selected into view
  const itemCallback = useCallback(
    (el: HTMLDivElement | null) => {
      if (el) el.scrollIntoView({ behavior: "smooth", block: "nearest" })
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [selectedRefId],
  )

  const resolveValue = (vid: string) => bundle.valuesById.get(vid)

  return (
    <div ref={scrollRef} className="flex flex-col">
      <div className="sticky top-0 z-10 border-b border-border bg-card px-3 py-2">
        <h3 className="text-[12px] font-semibold text-foreground/80">Références</h3>
        <div className="mt-1 flex flex-wrap gap-1.5">
          {Object.entries(REF_COLORS).map(([type, color]) => {
            const count = groups.get(type)?.length ?? 0
            if (count === 0) return null
            return (
              <span
                key={type}
                className="flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium"
                style={{ backgroundColor: color + "20", color }}
              >
                <span
                  className="size-2 rounded-full"
                  style={{ backgroundColor: color }}
                />
                {refLabel(type)} ({count})
              </span>
            )
          })}
        </div>
      </div>

      {[...groups.entries()].map(([type, refs]) => (
        <div key={type}>
          <div
            className="sticky top-[62px] z-[5] border-b border-border/50 px-3 py-1 text-[10.5px] font-semibold"
            style={{ backgroundColor: refColor(type) + "10", color: refColor(type) }}
          >
            {refLabel(type)}
          </div>
          {refs.map((ref) => {
            const subVal = resolveValue(ref.subject.value_id)
            const selected = ref.id === selectedRefId
            const subPage = subVal?.locator.page
            return (
              <div
                key={ref.id}
                ref={selected ? itemCallback : undefined}
                onClick={() => onSelectRef(ref.id, subPage ?? undefined)}
                className={cn(
                  "cursor-pointer border-b border-border/30 px-3 py-2 text-[11px] transition-colors",
                  selected ? "bg-[var(--signal)]/10" : "hover:bg-muted/40",
                )}
              >
                <div className="flex items-center gap-1.5">
                  <span
                    className="size-2 shrink-0 rounded-full"
                    style={{ backgroundColor: refColor(ref.type) }}
                  />
                  <span className="font-medium text-foreground/80 truncate">
                    {subVal?.text ?? ref.subject.value_id}
                  </span>
                  {ref.concordance != null &&
                    (ref.concordance ? (
                      <Check className="ml-auto size-3.5 shrink-0 text-[var(--ok)]" />
                    ) : (
                      <X className="ml-auto size-3.5 shrink-0 text-[var(--danger)]" />
                    ))}
                </div>

                {selected && (
                  <div className="mt-1.5 space-y-1 text-[10.5px] text-muted-foreground">
                    {subVal?.number != null && (
                      <Row label="Sujet" value={fmt(subVal.number)} />
                    )}
                    {ref.arrows.map((a, i) => {
                      const aVal = resolveValue(a.value_id)
                      return (
                        <div key={i} className="flex items-center gap-1">
                          <ArrowRight className="size-3 shrink-0" />
                          <span className="text-muted-foreground/60">
                            {a.sign === "-" ? "−" : "+"}{" "}
                          </span>
                          <span className="truncate">{aVal?.text ?? a.value_id}</span>
                          {aVal?.number != null && (
                            <span className="ml-auto tabular-nums">{fmt(aVal.number)}</span>
                          )}
                        </div>
                      )
                    })}
                    {ref.ecart != null && (
                      <Row
                        label="Écart"
                        value={fmt(ref.ecart)}
                        warn={!ref.concordance}
                      />
                    )}
                    {ref.comment && (
                      <p className="italic text-muted-foreground/60">{ref.comment}</p>
                    )}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      ))}
    </div>
  )
}

/* ── tiny helpers ──────────────────────────────────────────────── */

function Row({ label, value, warn }: { label: string; value: string; warn?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="text-muted-foreground/60">{label}</span>
      <span className={cn("tabular-nums", warn && "font-medium text-[var(--danger)]")}>
        {value}
      </span>
    </div>
  )
}

function fmt(n: number): string {
  return n.toLocaleString("fr-FR", { minimumFractionDigits: 0, maximumFractionDigits: 2 })
}
