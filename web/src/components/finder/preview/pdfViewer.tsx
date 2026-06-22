/**
 * Continuous-scroll PDF viewer built on pdf.js. Renders ALL pages
 * vertically in a scrollable container, each sized to fit the
 * container width. Supports overlay highlights with click handlers
 * and programmatic scroll-to-page via {@link scrollToPage}.
 */
import { useCallback, useEffect, useRef, useState } from "react"
import * as pdfjsLib from "pdfjs-dist"
import { cn } from "@/lib/utils"

/* ── pdf.js worker (shared, idempotent) ──────────────────────── */
if (!pdfjsLib.GlobalWorkerOptions.workerSrc) {
  pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
    "pdfjs-dist/build/pdf.worker.min.mjs",
    import.meta.url,
  ).href
}

/* ── types ─────────────────────────────────────────────────────── */

export interface Highlight {
  /** Unique key for React. */
  id: string
  /** 1-based page number. */
  page: number
  /** PyMuPDF bbox [x0, y0, x1, y1] in PDF points. */
  bbox: [number, number, number, number]
  /** CSS color for the highlight. */
  color: string
  /** Whether this highlight is currently selected. */
  selected?: boolean
  /** Tooltip on hover. */
  title?: string
}

export interface PdfViewerProps {
  /** Raw PDF bytes. */
  data: Uint8Array
  /** Overlay highlights. */
  highlights?: Highlight[]
  /** Called when a highlight is clicked. */
  onHighlightClick?: (id: string) => void
  /** Page to scroll to (1-based). Changes trigger scrollIntoView. */
  scrollToPage?: number
  /** Compact label shown in a sticky corner (e.g. "Source PDF"). */
  label?: string
}

/* ── constants ─────────────────────────────────────────────────── */
const PAGE_GAP = 8
const MIN_SCALE = 0.5
const MAX_SCALE = 4.0

/* ── component ─────────────────────────────────────────────────── */

export function PdfViewer({
  data,
  highlights = [],
  onHighlightClick,
  scrollToPage,
  label,
}: PdfViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const pageRefs = useRef<Map<number, HTMLDivElement>>(new Map())
  const [doc, setDoc] = useState<pdfjsLib.PDFDocumentProxy | null>(null)
  const [numPages, setNumPages] = useState(0)
  const [containerWidth, setContainerWidth] = useState(0)
  const [error, setError] = useState<string | null>(null)

  /* Load the PDF document */
  useEffect(() => {
    let cancelled = false
    setError(null)
    pdfjsLib
      .getDocument({ data })
      .promise.then((d) => {
        if (!cancelled) {
          setDoc(d)
          setNumPages(d.numPages)
        }
      })
      .catch((e) => {
        if (!cancelled) setError(e?.message ?? "Failed to load PDF")
      })
    return () => {
      cancelled = true
    }
  }, [data])

  /* Track container width for responsive scaling */
  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const ro = new ResizeObserver(([entry]) => {
      if (entry) setContainerWidth(entry.contentRect.width)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  /* Scroll to page when scrollToPage changes */
  useEffect(() => {
    if (!scrollToPage || scrollToPage < 1) return
    const el = pageRefs.current.get(scrollToPage)
    if (el) el.scrollIntoView({ behavior: "smooth", block: "center" })
  }, [scrollToPage])

  const registerPage = useCallback(
    (pageNum: number) => (el: HTMLDivElement | null) => {
      if (el) pageRefs.current.set(pageNum, el)
      else pageRefs.current.delete(pageNum)
    },
    [],
  )

  if (error) {
    return (
      <div className="flex flex-1 items-center justify-center text-xs text-destructive p-4">
        {error}
      </div>
    )
  }
  if (!doc) {
    return (
      <div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">
        Chargement…
      </div>
    )
  }

  return (
    <div ref={containerRef} className="relative min-h-0 flex-1 overflow-auto bg-neutral-100 dark:bg-neutral-900">
      {label && (
        <div className="sticky top-0 z-20 flex justify-end p-1">
          <span className="rounded bg-background/80 px-2 py-0.5 text-[10px] font-medium text-muted-foreground backdrop-blur">
            {label}
          </span>
        </div>
      )}
      <div className="flex flex-col items-center gap-[8px] p-4">
        {Array.from({ length: numPages }, (_, i) => i + 1).map((pageNum) => (
          <PageCanvas
            key={pageNum}
            ref={registerPage(pageNum)}
            doc={doc}
            pageNum={pageNum}
            containerWidth={containerWidth}
            highlights={highlights.filter((h) => h.page === pageNum)}
            onHighlightClick={onHighlightClick}
          />
        ))}
      </div>
    </div>
  )
}

/* ── single page canvas ────────────────────────────────────────── */

import { forwardRef, type Ref } from "react"

const PageCanvas = forwardRef(function PageCanvas(
  {
    doc,
    pageNum,
    containerWidth,
    highlights,
    onHighlightClick,
  }: {
    doc: pdfjsLib.PDFDocumentProxy
    pageNum: number
    containerWidth: number
    highlights: Highlight[]
    onHighlightClick?: (id: string) => void
  },
  ref: Ref<HTMLDivElement>,
) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [dims, setDims] = useState<{ w: number; h: number; scale: number } | null>(null)
  const renderTask = useRef<pdfjsLib.RenderTask | null>(null)

  useEffect(() => {
    if (containerWidth <= 0) return
    let cancelled = false

    ;(async () => {
      const page = await doc.getPage(pageNum)
      if (cancelled) return

      /* Compute scale to fit container width (minus padding) */
      const baseVp = page.getViewport({ scale: 1 })
      const availableWidth = Math.max(containerWidth - 32, 200)
      let scale = availableWidth / baseVp.width
      scale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, scale))

      const viewport = page.getViewport({ scale })
      setDims({ w: viewport.width, h: viewport.height, scale })

      const canvas = canvasRef.current
      if (!canvas || cancelled) return

      /* Cancel any previous render */
      if (renderTask.current) {
        try { renderTask.current.cancel() } catch { /* ignore */ }
      }

      canvas.width = viewport.width
      canvas.height = viewport.height
      const ctx = canvas.getContext("2d")
      if (!ctx) return

      /* Clear first to avoid flash of stale content */
      ctx.clearRect(0, 0, viewport.width, viewport.height)

      const task = page.render({ canvasContext: ctx, viewport })
      renderTask.current = task
      try {
        await task.promise
      } catch {
        /* cancelled — ignore */
      }
    })()

    return () => {
      cancelled = true
    }
  }, [doc, pageNum, containerWidth])

  return (
    <div ref={ref} className="relative" style={dims ? { width: dims.w, height: dims.h } : undefined}>
      <canvas ref={canvasRef} className="block rounded shadow-md" />
      {dims &&
        highlights.map((h) => {
          const [x0, y0, x1, y1] = h.bbox
          const s = dims.scale
          return (
            <div
              key={h.id}
              onClick={() => onHighlightClick?.(h.id)}
              title={h.title}
              className={cn(
                "absolute cursor-pointer rounded-sm border-2 transition-all",
                h.selected ? "ring-2 ring-offset-1 z-10" : "hover:brightness-110",
              )}
              style={{
                left: x0 * s,
                top: y0 * s,
                width: (x1 - x0) * s,
                height: (y1 - y0) * s,
                backgroundColor: h.color + "30",
                borderColor: h.color,
                ringColor: h.color,
              }}
            />
          )
        })}
    </div>
  )
})
