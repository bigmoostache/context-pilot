/**
 * Integrated Finder preview for `.sourced_pdf` bundles — the v3
 * split-view cross-reference auditor. Left: target PDF with continuous
 * scroll and subject highlights. Right: the actual source document
 * (PDF via continuous viewer, or Excel values table) when a ref is
 * selected.
 */
import { useEffect, useMemo, useState } from "react"
import { Check, X, FileSpreadsheet } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { rawUrl } from "@/lib/api"
import { cn } from "@/lib/utils"
import { PreviewStatus } from "./previewParts"
import { PdfViewer, type Highlight } from "./pdfViewer"
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

/* ── main component ────────────────────────────────────────────── */

export function LiveSourcedPdfPreview({
  agentId,
  node,
}: {
  agentId: string
  node: FinderNode
}) {
  const [bundle, setBundle] = useState<SourcedBundle | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [selectedRefId, setSelectedRefId] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const resp = await fetch(rawUrl(agentId, node.path))
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`)
        const bytes = await resp.arrayBuffer()
        const b = await parseSourcedBundle(bytes)
        if (!cancelled) setBundle(b)
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : "Load failed")
      }
    })()
    return () => { cancelled = true }
  }, [agentId, node.path])

  if (error) return <PreviewStatus label={`Error: ${error}`} />
  if (!bundle) return <PreviewStatus label="Loading sourced PDF…" />

  const selectedRef = selectedRefId
    ? bundle.refs.find((r) => r.id === selectedRefId) ?? null
    : null

  const arrowDoc = selectedRef?.arrows[0]
    ? bundle.metadata.documents.find((d) => d.id === selectedRef.arrows[0].document_id)
    : null

  return (
    <div className="flex h-full min-h-0 flex-col">
      <NavBar bundle={bundle} />
      {selectedRef && (
        <RefStrip ref_={selectedRef} bundle={bundle} onClose={() => setSelectedRefId(null)} />
      )}
      <div className="flex min-h-0 flex-1">
        {/* LEFT: target PDF with continuous scroll */}
        <div className="flex min-w-0 flex-1 flex-col border-r border-border">
          <TargetPdf
            bundle={bundle}
            selectedRefId={selectedRefId}
            onSelectRef={setSelectedRefId}
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

function NavBar({ bundle }: { bundle: SourcedBundle }) {
  const concordant = bundle.refs.filter((r) => r.concordance).length
  const total = bundle.refs.length
  const hasConcordance = bundle.refs.some((r) => r.concordance != null)
  const pct = total > 0 ? Math.round((concordant / total) * 100) : 0
  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-border bg-muted/40 px-3 py-1.5">
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
      {hasConcordance && (
        <span className={cn("ml-auto text-[10.5px] font-medium",
          pct >= 95 ? "text-[var(--ok)]" : "text-[var(--warn)]")}>
          {pct}% concordant
        </span>
      )}
      {!hasConcordance && (
        <span className="ml-auto text-[10px] text-muted-foreground/50">
          {total} refs
        </span>
      )}
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

/* ── target PDF (left side) ────────────────────────────────────── */

function TargetPdf({
  bundle, selectedRefId, onSelectRef,
}: {
  bundle: SourcedBundle
  selectedRefId: string | null
  onSelectRef: (id: string | null) => void
}) {
  const highlights = useMemo<Highlight[]>(() => {
    const targetId = bundle.metadata.target_doc_id
    const subjectMap = new Map<string, Ref>()
    for (const ref of bundle.refs) {
      if (ref.subject.document_id === targetId)
        subjectMap.set(ref.subject.value_id, ref)
    }
    const result: Highlight[] = []
    for (const v of bundle.values) {
      if (v.document_id !== targetId || !v.locator.bbox || !v.locator.page) continue
      const ref = subjectMap.get(v.id)
      if (!ref) continue
      result.push({
        id: ref.id,
        page: v.locator.page,
        bbox: v.locator.bbox,
        color: refColor(ref.type),
        selected: ref.id === selectedRefId,
        title: `${v.text} — ${refLabel(ref.type)}`,
      })
    }
    return result
  }, [bundle, selectedRefId])

  return (
    <PdfViewer
      data={bundle.targetDocBytes}
      highlights={highlights}
      onHighlightClick={(id) => onSelectRef(id === selectedRefId ? null : id)}
      label="Document cible"
    />
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
        <p className="text-center leading-relaxed">
          Cliquez sur un highlight<br/>pour afficher le document source
        </p>
      </div>
    )
  }

  const sourceBytes = bundle.docBytes.get(arrowDoc.id)
  if (!sourceBytes) {
    return <PreviewStatus label={`Source "${arrowDoc.path}" introuvable`} />
  }

  const type = docType(arrowDoc.path)
  const arrowValue = bundle.valuesById.get(selectedRef.arrows[0]?.value_id)

  if (type === "pdf") {
    /* Build highlights for ALL arrows of this ref in this source doc */
    const sourceHighlights: Highlight[] = selectedRef.arrows
      .map((a) => bundle.valuesById.get(a.value_id))
      .filter((v): v is Value => !!v && !!v.locator.bbox && !!v.locator.page)
      .map((v) => ({
        id: v.id,
        page: v.locator.page!,
        bbox: v.locator.bbox!,
        color: refColor(selectedRef.type),
        selected: true,
        title: v.text,
      }))

    return (
      <PdfViewer
        data={sourceBytes}
        highlights={sourceHighlights}
        scrollToPage={arrowValue?.locator.page}
        label="Source PDF"
      />
    )
  }

  /* Excel / unknown → values table */
  return <ExcelSourceTable doc={arrowDoc} ref_={selectedRef} bundle={bundle} />
}

/* ── Excel source table ────────────────────────────────────────── */

function ExcelSourceTable({
  doc, ref_, bundle,
}: {
  doc: DocEntry; ref_: Ref; bundle: SourcedBundle
}) {
  /* All values from this document, grouped by sheet */
  const grouped = useMemo(() => {
    const docValues = bundle.values.filter((v) => v.document_id === doc.id)
    const groups = new Map<string, Value[]>()
    for (const v of docValues) {
      const sheet = v.locator.sheet ?? "(default)"
      const arr = groups.get(sheet) ?? []
      arr.push(v)
      groups.set(sheet, arr)
    }
    return groups
  }, [bundle, doc.id])

  const arrowValueIds = new Set(ref_.arrows.map((a) => a.value_id))

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-muted/30 px-3 py-1.5">
        <FileSpreadsheet className="size-4 text-[var(--ok)]" />
        <span className="text-[11px] font-semibold text-foreground/80 truncate">
          {doc.path.split("/").pop()}
        </span>
        <span className="ml-auto text-[10px] text-muted-foreground/50">
          {bundle.values.filter((v) => v.document_id === doc.id).length} valeurs
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-3">
        {[...grouped.entries()].map(([sheet, vals]) => (
          <div key={sheet} className="mb-4">
            <h4 className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/60">
              {sheet}
            </h4>
            <div className="overflow-hidden rounded-lg border border-border/50">
              <table className="w-full text-[11px]">
                <thead>
                  <tr className="border-b border-border/30 bg-muted/20">
                    <th className="px-2 py-1 text-left font-medium text-muted-foreground">Cellule</th>
                    <th className="px-2 py-1 text-left font-medium text-muted-foreground">Texte</th>
                    <th className="px-2 py-1 text-right font-medium text-muted-foreground">Nombre</th>
                  </tr>
                </thead>
                <tbody>
                  {vals.map((v) => {
                    const isArrow = arrowValueIds.has(v.id)
                    return (
                      <tr key={v.id}
                        className={cn(
                          "border-b border-border/20 last:border-0",
                          isArrow && "bg-[color-mix(in_srgb,var(--ok)_10%,transparent)]",
                        )}>
                        <td className="px-2 py-1 tabular-nums text-muted-foreground/70">
                          {v.locator.cell ?? "—"}
                        </td>
                        <td className={cn("px-2 py-1 truncate max-w-[200px]",
                          isArrow && "font-semibold text-foreground")}>
                          {v.text}
                        </td>
                        <td className={cn("px-2 py-1 text-right tabular-nums",
                          isArrow && "font-semibold text-foreground")}>
                          {v.number != null ? fmt(v.number) : "—"}
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

/* ── tiny helpers ──────────────────────────────────────────────── */

function fmt(n: number): string {
  return n.toLocaleString("fr-FR", { minimumFractionDigits: 0, maximumFractionDigits: 2 })
}
