import { useMemo, useState } from "react"
import { Check, Copy, Pencil, Save, X } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { useFsPreview, useFsSheet, useWriteFile } from "@/lib/live"
import { rawUrl } from "@/lib/api"
import { highlightCode } from "./codeHighlight"
import { MarkdownEditor } from "@/components/agents/MarkdownEditor"
import { FileIcon } from "../support/macIcons"
import { cn } from "@/lib/utils"
import { Generic, MarkdownPreview, PreviewStatus, TextPreview, TruncatedNote } from "./previewParts"

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

// ── live spreadsheet (csv / xlsx / ods → table) ───────────────────
/**
 * Render a LIVE spreadsheet as a table from the backend's `/fs/sheet` endpoint
 * (CSV/TSV parsed with quote handling, xlsx/xls/ods via calamine). Multi-sheet
 * workbooks get a tab switcher; each sheet treats its first row as a sticky
 * header and zebra-stripes the body, with a left row-number gutter and
 * horizontal scroll for wide sheets. A clipped (capped) payload shows a note;
 * an unparseable file or read fault falls back to the no-preview state.
 */
export function LiveSheetPreview({ agentId, node }: { agentId: string; node: FinderNode }) {
  const { data, loading, error } = useFsSheet(agentId, node.path, true)
  const [active, setActive] = useState(0)
  if (loading) return <PreviewStatus label="Loading spreadsheet…" />
  if (error || !data || data.sheets.length === 0) return <Generic node={node} />

  const sheet = data.sheets[Math.min(active, data.sheets.length - 1)]
  const rows = sheet?.rows ?? []
  // First row = header (mirrors how a spreadsheet shows row 1); the rest are
  // data rows. An empty sheet shows just the note.
  const header = rows[0] ?? []
  const body = rows.slice(1)
  // Pad every row to the widest so ragged rows (the backend doesn't square the
  // grid) stay column-aligned.
  const cols = rows.reduce((m, r) => Math.max(m, r.length), 0)

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex flex-col overflow-hidden">
        <div className="overflow-auto">
          <table className="w-full border-collapse text-[11px]">
            <thead className="sticky top-0">
              <tr>
                <th className="w-9 border border-border bg-muted/70 p-1 text-center text-[10px] text-muted-foreground/50">
                  #
                </th>
                {Array.from({ length: cols }, (_, c) => (
                  <th
                    key={c}
                    className="border border-border bg-(--ok)/10 px-2 py-1.5 text-left font-semibold text-foreground/85"
                  >
                    {header[c] ?? ""}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {body.map((row, r) => (
                <tr key={r} className={cn(r % 2 === 1 && "bg-muted/20", "hover:bg-(--signal)/8")}>
                  <td className="border border-border bg-muted/50 p-1 text-center text-[10px] text-muted-foreground/50">
                    {r + 2}
                  </td>
                  {Array.from({ length: cols }, (_, c) => (
                    <td
                      key={c}
                      className={cn(
                        "border border-border px-2 py-1 text-foreground/80 tabular-nums",
                        c === 0 && "font-medium",
                      )}
                    >
                      {row[c] ?? ""}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {/* sheet tabs — only for a multi-sheet workbook */}
        {data.sheets.length > 1 && (
          <div className="flex items-center gap-1 overflow-x-auto border-t border-border bg-muted/40 px-2 py-1">
            {data.sheets.map((s, i) => (
              <button
                key={`${s.name}${i}`}
                onClick={() => setActive(i)}
                className={cn(
                  "shrink-0 rounded-t-md px-2.5 py-0.5 text-[10.5px] transition-colors",
                  i === active
                    ? "border-x border-t border-border bg-card font-medium text-foreground/80"
                    : "text-muted-foreground/60 hover:text-foreground/80",
                )}
              >
                {s.name}
              </button>
            ))}
          </div>
        )}
      </div>
      {data.truncated && (
        <p className="text-[10.5px] text-muted-foreground/60 italic">
          Preview clipped — large sheet capped at 1000 rows × 50 columns.
        </p>
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
 * A LIVE markdown file with a view ⇄ WYSIWYG-edit toggle (X906). View mode shows
 * the rendered markdown plus an Edit affordance; edit mode swaps in the
 * {@link MarkdownEditor} (a contentEditable WYSIWYG surface) seeded with the
 * file's text and reports its serialized markdown on every keystroke. Save
 * writes the draft back to the file via `fs/write`, then re-renders the saved
 * content; Cancel discards.
 *
 * Editing is disabled for a TRUNCATED preview: the backend caps the preview at
 * 256 KiB, so the editor would only hold the head of the file — saving it would
 * silently drop the tail. Such files stay read-only with an explanatory note.
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
  const [editing, setEditing] = useState(false)
  // Draft markdown serialized live from the editor; seeded with the file text so
  // a Save with no edits is a harmless no-op write of the same content.
  const [draft, setDraft] = useState(content)
  const write = useWriteFile(agentId)
  const [err, setErr] = useState<string | null>(null)

  if (!editing) {
    return (
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-end">
          {truncated ? (
            <span className="text-[10.5px] text-muted-foreground/60 italic">
              Editing disabled — file exceeds the 256 KiB preview cap.
            </span>
          ) : (
            <button
              onClick={() => {
                setDraft(content)
                setErr(null)
                setEditing(true)
              }}
              className="flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[11.5px] text-foreground/80 transition-colors hover:bg-muted hover:text-foreground"
            >
              <Pencil className="size-3" />
              Edit
            </button>
          )}
        </div>
        <MarkdownPreview text={content} truncated={truncated ?? false} />
      </div>
    )
  }

  const save = () => {
    setErr(null)
    write.mutate(
      { path, content: draft },
      {
        onSuccess: () => setEditing(false),
        onError: (e) => setErr(e instanceof Error ? e.message : "Save failed"),
      },
    )
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex items-center gap-2">
        <span className="text-[11.5px] font-medium text-muted-foreground">Editing — Markdown</span>
        {err && <span className="truncate text-[11px] text-(--danger)">{err}</span>}
        <div className="ml-auto flex items-center gap-1.5">
          <button
            onClick={() => setEditing(false)}
            disabled={write.isPending}
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[11.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
          >
            <X className="size-3" />
            Cancel
          </button>
          <button
            onClick={save}
            disabled={write.isPending}
            className="flex items-center gap-1.5 rounded-md bg-(--signal) px-2.5 py-1 text-[11.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:opacity-60"
          >
            <Save className="size-3" />
            {write.isPending ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
      <MarkdownEditor initialMarkdown={content} onChange={setDraft} className="min-h-[280px]" />
    </div>
  )
}
