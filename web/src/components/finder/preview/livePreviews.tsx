import { useMemo, useRef, useState, lazy, Suspense } from "react"
import { Check, Copy, Save } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { useFsPreview, useFsSheet, useWriteFile } from "@/lib/live"
import { rawUrl } from "@/lib/api"
import { highlightCode } from "./codeHighlight"
/** Lazy-loaded — TipTap + ProseMirror deferred until a markdown file is edited. */
const LazyMarkdownEditor = lazy(() =>
  import("@/components/agents/MarkdownEditor").then((m) => ({ default: m.MarkdownEditor })),
)
import { FileIcon } from "../support/macIcons"
import { Generic, MarkdownPreview, PreviewStatus, TextPreview, TruncatedNote } from "./previewParts"
/** Lazy-loaded — Univer + ExcelJS (~46 MB) deferred until a sheet is previewed. */
const LazySheetGrid = lazy(() =>
  import("./SheetGrid").then((m) => ({ default: m.SheetGrid })),
)

/**
 * Fetch a live file's text content and render it: markdown through the rich GFM
 * renderer, everything else as a preformatted block. While loading shows a quiet
 * placeholder; a binary file (415) or read fault resolves the fetch as an error,
 * which falls back to the honest "No preview available" state.
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
 * Render a LIVE image straight from the backend's inline raw-serve endpoint
 * (`/fs/raw`). The `<img>` loads the real bytes; `onLoad` captures the natural
 * pixel dimensions for the footer, and a failed load (oversized / unreadable /
 * decode error) flips to the honest no-preview fallback. A checkerboard backing
 * shows transparency, and a zoom control mirrors the mock image preview.
 */
export function LiveImagePreview({ agentId, node }: { agentId: string; node: FinderNode }) {
  const [zoom, setZoom] = useState(100)
  const [dims, setDims] = useState<{ w: number; h: number } | null>(null)
  const [failed, setFailed] = useState(false)
  const src = useMemo(() => rawUrl(agentId, node.path), [agentId, node.path])
  if (failed) return <Generic node={node} />
  return (
    <div className="flex flex-col gap-2">
      <div className="checker overflow-hidden">
        <div className="flex items-center justify-center p-4">
          <img
            src={src}
            alt={node.name}
            onLoad={(e) =>
              setDims({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })
            }
            onError={() => setFailed(true)}
            className="card-shadow max-h-[420px] max-w-full rounded-md object-contain transition-transform"
            style={{ transform: `scale(${zoom / 100})` }}
          />
        </div>
      </div>
      <div className="flex items-center gap-2">
        <span className="font-mono text-[11px] text-muted-foreground">
          {dims ? `${dims.w} × ${dims.h}` : "—"}
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          <button
            onClick={() => setZoom((z) => Math.max(25, z - 25))}
            className="flex size-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-muted hover:text-foreground"
          >
            −
          </button>
          <span className="w-9 text-center text-[11px] text-muted-foreground tabular-nums">
            {zoom}%
          </span>
          <button
            onClick={() => setZoom((z) => Math.min(400, z + 25))}
            className="flex size-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-muted hover:text-foreground"
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
 * Render a LIVE PDF inline via the browser's native viewer, pointed at the
 * backend's inline raw-serve endpoint (`/fs/raw`). `<object>` embeds the PDF;
 * its child is the fallback the browser shows when it can't render PDFs inline
 * (or the file failed to load) — a link that opens the raw bytes in a new tab.
 */
export function LivePdfPreview({ agentId, node }: { agentId: string; node: FinderNode }) {
  const src = useMemo(() => rawUrl(agentId, node.path), [agentId, node.path])
  return (
    // h-full fills the (definite-height) scroll container so the embedded PDF
    // viewer claims ALL available height in the pane / file tab — flex-1 alone
    // had no effect because the scroll parent is a plain block, not a flex
    // column, so the object collapsed to its min-height.
    <div className="flex h-full min-h-0 flex-col gap-2">
      <object data={src} type="application/pdf" className="min-h-0 w-full flex-1 bg-card">
        <div className="flex flex-col items-center gap-3 py-10 text-center">
          <FileIcon kind="pdf" size={64} />
          <span className="text-[12.5px] text-muted-foreground">
            Inline PDF preview isn’t available here.
          </span>
          <a
            href={src}
            target="_blank"
            rel="noreferrer"
            className="rounded-md bg-(--signal) px-3 py-1.5 text-[12px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105"
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
 * Render a LIVE spreadsheet via Univer from the backend's `/fs/sheet`
 * endpoint (CSV/TSV parsed with quote handling, xlsx/xls/ods via calamine).
 * Univer provides a full spreadsheet engine: range selection, formatting,
 * formulas, sheet tabs, freeze panes, merge cells — all from the free
 * Apache 2.0 tier. XLSX download via ExcelJS reflects all edits.
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

      {/* truncation status */}
      {data.truncated && (
        <div className="flex items-center border-t border-border bg-muted/20 px-3 py-1">
          <span className="text-[10.5px] text-muted-foreground/50 italic">
            Preview clipped — large sheet capped at 1 000 rows × 50 columns
          </span>
        </div>
      )}
    </div>
  )
}

// ── highlighted code (live files) ─────────────────────────────────
/**
 * Real syntax-highlighted code preview for a LIVE file. Resolves the language
 * from the filename, highlights via highlight.js (see codeHighlight.ts), and
 * lays the result out with the macOS code-window chrome: traffic lights, a
 * language label, a copy button, and per-line gutter numbers.
 *
 * The highlighted HTML is split on newlines so each source line keeps its own
 * gutter number while still carrying its highlight spans. highlight.js never
 * leaves a tag open across a newline, so splitting the rendered HTML on `\n`
 * yields self-contained per-line fragments that are safe to inject.
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
      <div className="flex items-center gap-2 border-b border-border bg-muted/50 px-3 py-1.5">
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
          className="ml-auto flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10.5px] text-muted-foreground/80 transition-colors hover:bg-muted hover:text-foreground"
        >
          {copied ? <Check className="size-3 text-(--ok)" /> : <Copy className="size-3" />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="hljs overflow-x-auto bg-transparent px-3 py-2.5 font-mono text-[11px] leading-relaxed">
        {lines.map((line, i) => (
          <div key={i} className="flex gap-3 rounded-sm hover:bg-(--signal)/6">
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
 * A LIVE markdown file rendered as an always-on TipTap WYSIWYG editor. No
 * view/edit toggle — the editing surface IS the preview. A single merged bar
 * holds formatting tools on the left and autosave status + manual Save on the
 * right. Autosave fires 1 s after the last keystroke. Truncated files
 * (>256 KiB preview cap) fall back to read-only {@link MarkdownPreview}.
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
        <span className="text-[10.5px] text-muted-foreground/70 italic">Saving…</span>
      )}
      {status === "saved" && (
        <span className="text-[10.5px] text-(--ok)">
          <Check className="inline size-3" /> Saved
        </span>
      )}
      {status === "error" && (
        <span className="truncate text-[10.5px] text-(--danger)">{err ?? "Error"}</span>
      )}
      <button
        title="Save"
        onClick={manualSave}
        disabled={write.isPending}
        className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/70 hover:text-foreground"
      >
        <Save className="size-3.5" />
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
