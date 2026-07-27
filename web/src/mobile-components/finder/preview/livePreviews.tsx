import { useMemo, useRef, useState, lazy, Suspense } from "react"
import { Check, Copy, Save } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { useFsPreview, useFsSheet, useWriteFile } from "@/lib/live"
import { rawUrl } from "@/lib/api"
import { highlightCode } from "./codeHighlight"
/** Lazy-loaded — TipTap + ProseMirror deferred until a markdown file is edited.
 *  Points at the MOBILE MarkdownEditor twin (@/mobile-components) — the leak
 *  guard forbids reaching back into @/components from a hand-authored mobile
 *  file, and the mobile editor is the touch-sized toolbar variant. */
const LazyMarkdownEditor = lazy(() =>
  import("@/mobile-components/agents/MarkdownEditor").then((m) => ({ default: m.MarkdownEditor })),
)
import { FileIcon } from "../support/macIcons"
import { Generic, MarkdownPreview, PreviewStatus, TextPreview, TruncatedNote } from "./previewParts"
/** Lazy-loaded — Univer + ExcelJS (~46 MB) deferred until a sheet is previewed.
 *  Resolves to the mobile SheetGrid (a stub re-exporting the desktop Univer
 *  wrapper: the engine handles touch natively, so it is not forked). */
const LazySheetGrid = lazy(() => import("./SheetGrid").then((m) => ({ default: m.SheetGrid })))

/**
 * Fetch a live file's text content and render it: markdown through the rich GFM
 * renderer, everything else as a preformatted block. Mobile twin — behaviour is
 * identical to desktop; the touch divergence lives in the leaf renderers below
 * (zoom / copy / save buttons become thumb-sized).
 */
export function LivePreview({ agentId, node }: { agentId: string; node: FinderNode }) {
  const { data, loading, error } = useFsPreview(agentId, node.path, true)
  if (loading) return <PreviewStatus label="Loading preview…" />
  if (error || !data) return <Generic node={node} />
  if (node.kind === "markdown")
    return (
      <EditableMarkdown
        agentId={agentId}
        path={node.path}
        content={data.content}
        truncated={data.truncated}
      />
    )
  if (node.kind === "code")
    return <HighlightedCode name={node.name} code={data.content} truncated={data.truncated} />
  return <TextPreview kind={node.kind} text={data.content} truncated={data.truncated} />
}

// ── live image (real bytes) ───────────────────────────────────────
/**
 * Mobile LIVE image preview. Same real-bytes `<img>` off the `/fs/raw` endpoint
 * as desktop; the zoom control is the divergence — desktop uses 20px (size-5)
 * hover buttons, here they are 36px (size-9) `active:`-press touch targets.
 */
export function LiveImagePreview({ agentId, node }: { agentId: string; node: FinderNode }) {
  const [zoom, setZoom] = useState(100)
  const [dims, setDims] = useState<{ w: number; h: number } | null>(null)
  const [failed, setFailed] = useState(false)
  const src = useMemo(() => rawUrl(agentId, node.path), [agentId, node.path])
  if (failed) return <Generic node={node} />
  return (
    <div className="flex flex-col gap-2 p-2">
      <div className="checker overflow-hidden rounded-md">
        <div className="flex items-center justify-center p-3">
          <img
            src={src}
            alt={node.name}
            onLoad={(e) =>
              setDims({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })
            }
            onError={() => setFailed(true)}
            className="card-shadow max-h-[60vh] max-w-full rounded-md object-contain transition-transform"
            style={{ transform: `scale(${zoom / 100})` }}
          />
        </div>
      </div>
      <div className="flex items-center gap-2 px-1">
        <span className="font-mono text-[12px] text-muted-foreground">
          {dims ? `${dims.w} × ${dims.h}` : "—"}
        </span>
        <div className="ml-auto flex items-center gap-1">
          <button
            onClick={() => setZoom((z) => Math.max(25, z - 25))}
            aria-label="Zoom out"
            className="flex size-9 items-center justify-center rounded-md text-muted-foreground active:bg-muted active:text-foreground"
          >
            −
          </button>
          <span className="w-10 text-center text-[12px] text-muted-foreground tabular-nums">
            {zoom}%
          </span>
          <button
            onClick={() => setZoom((z) => Math.min(400, z + 25))}
            aria-label="Zoom in"
            className="flex size-9 items-center justify-center rounded-md text-muted-foreground active:bg-muted active:text-foreground"
          >
            +
          </button>
        </div>
      </div>
    </div>
  )
}

// ── live PDF (real bytes) ─────────────────────────────────────────
/**
 * Mobile LIVE PDF — same native `<object>` viewer off `/fs/raw` as desktop.
 * Mobile browsers frequently can't render a PDF inline, so the child fallback
 * (a full-width "Open PDF" button) matters more here; it's sized as a real
 * touch target.
 */
export function LivePdfPreview({ agentId, node }: { agentId: string; node: FinderNode }) {
  const src = useMemo(() => rawUrl(agentId, node.path), [agentId, node.path])
  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      <object data={src} type="application/pdf" className="min-h-0 w-full flex-1 bg-card">
        <div className="flex flex-col items-center gap-3 py-10 text-center">
          <FileIcon kind="pdf" size={64} />
          <span className="text-[13px] text-muted-foreground">
            Inline PDF preview isn’t available here.
          </span>
          <a
            href={src}
            target="_blank"
            rel="noreferrer"
            className="rounded-md bg-(--signal) px-4 py-2.5 text-[14px] font-medium text-(--primary-foreground) active:brightness-105"
          >
            Open PDF in new tab
          </a>
        </div>
      </object>
    </div>
  )
}

// ── live spreadsheet (csv / xlsx / ods → Univer) ─────────────────
/**
 * Mobile LIVE spreadsheet — lazy-loads the (unforked) Univer SheetGrid. Univer
 * ships its own touch handling (pan/select/scroll), so the engine is shared;
 * only the surrounding truncation note is mobile-sized here.
 */
export function LiveSheetPreview({ agentId, node }: { agentId: string; node: FinderNode }) {
  const { data, loading, error } = useFsSheet(agentId, node.path, true)
  if (loading) return <PreviewStatus label="Loading spreadsheet…" />
  if (error || !data || data.sheets.length === 0) return <Generic node={node} />

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Suspense fallback={<PreviewStatus label="Loading spreadsheet…" />}>
        <LazySheetGrid sheets={data.sheets} path={node.path} agentId={agentId} />
      </Suspense>

      {data.truncated && (
        <div className="flex items-center border-t border-border bg-muted/20 px-3 py-1.5">
          <span className="text-[11px] text-muted-foreground/50 italic">
            Preview clipped — large sheet capped at 1 000 rows × 50 columns
          </span>
        </div>
      )}
    </div>
  )
}

// ── highlighted code (live files) ─────────────────────────────────
/**
 * Mobile syntax-highlighted code preview. Same highlight.js render + per-line
 * gutter as desktop; the copy button is the touch fork — a 36px `active:` chip
 * instead of a hover-revealed one — and the per-line hover tint is dropped
 * (no hover on touch).
 */
function HighlightedCode({
  name,
  code,
  truncated,
}: {
  name: string
  code: string
  truncated?: boolean
}) {
  const { html, language } = useMemo(() => highlightCode(code, name), [code, name])
  const lines = useMemo(() => html.split("\n"), [html])
  const [copied, setCopied] = useState(false)
  const copy = () => {
    ;(navigator.clipboard as Clipboard | undefined)?.writeText(code).catch(() => {
      /* clipboard write may reject on insecure origin — ignore, the tick just won't flash */
    })
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1400)
  }
  return (
    <div className="bg-card">
      <div className="flex items-center gap-2 border-b border-border bg-muted/50 px-3 py-2">
        <span className="flex gap-1.5">
          <span className="size-2.5 rounded-full bg-[#ff5f57]" />
          <span className="size-2.5 rounded-full bg-[#febc2e]" />
          <span className="size-2.5 rounded-full bg-[#28c840]" />
        </span>
        <span className="ml-1 font-mono text-[10.5px] tracking-wide text-muted-foreground uppercase">
          {language}
        </span>
        <button
          onClick={copy}
          className="ml-auto flex items-center gap-1 rounded-md px-2.5 py-1.5 text-[12px] text-muted-foreground/80 transition-colors active:bg-muted active:text-foreground"
        >
          {copied ? <Check className="size-3.5 text-(--ok)" /> : <Copy className="size-3.5" />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="hljs overflow-x-auto bg-transparent px-3 py-2.5 font-mono text-[12px] leading-relaxed">
        {lines.map((line, i) => (
          <div key={i} className="flex gap-3 rounded-sm">
            <span className="w-7 shrink-0 text-right text-muted-foreground/35 select-none">
              {i + 1}
            </span>
            <code
              className="min-w-0 whitespace-pre"
              // highlight.js escapes the source; the markup is class-tagged spans only.
              dangerouslySetInnerHTML={{ __html: line || "\u{200B}" }}
            />
          </div>
        ))}
      </pre>
      {truncated && (
        <div className="px-3 pb-2">
          <TruncatedNote />
        </div>
      )}
    </div>
  )
}

/**
 * Mobile LIVE markdown editor — always-on TipTap WYSIWYG (the editing surface IS
 * the preview) via the mobile MarkdownEditor twin. Same 1 s-debounce autosave +
 * manual Save as desktop; the Save button is a 40px touch target and truncated
 * files fall back to the read-only mobile {@link MarkdownPreview}.
 */
function EditableMarkdown({
  agentId,
  path,
  content,
  truncated,
}: {
  agentId: string
  path: string
  content: string
  truncated?: boolean
}) {
  const draftRef = useRef(content)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const write = useWriteFile(agentId)
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle")
  const [err, setErr] = useState<string | null>(null)

  // Truncated files stay read-only — editing would lose the tail.
  if (truncated) {
    return <MarkdownPreview text={content} truncated />
  }

  const doSave = (md: string) => {
    if (md === content) return
    setStatus("saving")
    setErr(null)
    write.mutate(
      { path, content: md },
      {
        onSuccess: () => {
          setStatus("saved")
          window.setTimeout(() => setStatus("idle"), 1500)
        },
        onError: (e) => {
          setStatus("error")
          setErr(e instanceof Error ? e.message : "Save failed")
        },
      },
    )
  }

  const onChange = (md: string) => {
    draftRef.current = md
    if (timerRef.current) clearTimeout(timerRef.current)
    timerRef.current = setTimeout(() => doSave(md), 1000)
  }

  const manualSave = () => {
    if (timerRef.current) clearTimeout(timerRef.current)
    doSave(draftRef.current)
  }

  const extra = (
    <div className="flex items-center gap-1.5">
      {status === "saving" && (
        <span className="text-[11px] text-muted-foreground/70 italic">Saving…</span>
      )}
      {status === "saved" && (
        <span className="text-[11px] text-(--ok)">
          <Check className="inline size-3.5" /> Saved
        </span>
      )}
      {status === "error" && (
        <span className="truncate text-[11px] text-(--danger)">{err ?? "Error"}</span>
      )}
      <button
        title="Save"
        aria-label="Save"
        onClick={manualSave}
        disabled={write.isPending}
        className="flex size-10 items-center justify-center rounded-md text-muted-foreground/70 transition-colors active:bg-muted/70 active:text-foreground"
      >
        <Save className="size-4" />
      </button>
    </div>
  )

  return (
    <Suspense fallback={<PreviewStatus label="Loading editor…" />}>
      <LazyMarkdownEditor
        initialMarkdown={content}
        onChange={onChange}
        toolbarExtra={extra}
        className="min-h-0 flex-1 rounded-none border-0"
      />
    </Suspense>
  )
}
