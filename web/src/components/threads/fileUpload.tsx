import { Paperclip, AlertTriangle } from "lucide-react"
import { kindOf, extOf } from "@/components/finder/support/kind"
import { FileIcon } from "@/components/finder/support/macIcons"
import { useFs } from "@/lib/live"
import type { FinderNode } from "@/lib/types"

/**
 * One file attached to a thread via the chat composer. The composer uploads the
 * file to the realm's `.uploads/` and embeds these fields into the user message
 * as a ` ```file-upload ` YAML block (one block per file); the conversation view
 * parses the blocks back out and renders each as a clickable {@link FileUploadChip}.
 */
export interface UploadedFile {
  /** realm-relative stored path, e.g. `.uploads/report (1).pdf` */
  path: string
  /** stored filename */
  name: string
  /** byte count */
  size: number
  /** provenance note, e.g. `uploaded by user at 2026-…` */
  note: string
}

/**
 * Compose a user message body carrying one ` ```file-upload ` YAML block per
 * uploaded file. The conversation renderer ({@link splitMessageSegments})
 * extracts these blocks and turns them into clickable preview chips rendered
 * **inline at the block's position**; the agent reads the same YAML as plain
 * context, so it knows which files were attached.
 */
export function buildUploadMessage(files: UploadedFile[]): string {
  return files
    .map((f) =>
      [
        "```file-upload",
        "file:",
        `  path: ${f.path}`,
        `  name: ${f.name}`,
        `  size: ${f.size}`,
        `  note: ${f.note}`,
        "```",
      ].join("\n"),
    )
    .join("\n\n")
}

const BLOCK_RE = /```file-upload\n([\s\S]*?)```/g

/** Pull one `key: value` out of a `file-upload` block body (indented under `file:`). */
function field(body: string, key: string): string {
  const m = body.match(new RegExp(`^\\s*${key}:\\s*(.*)$`, "m"))
  return m ? m[1].trim() : ""
}

/** Parse one `file-upload` block body into an {@link UploadedFile} (tolerant:
 *  a missing `name` falls back to the path's basename, a missing `size` to 0). */
function parseBlock(body: string): UploadedFile | null {
  const path = field(body, "path")
  if (!path) return null
  return {
    path,
    name: field(body, "name") || path.split("/").pop() || path,
    size: Number(field(body, "size")) || 0,
    note: field(body, "note"),
  }
}

/** An ordered render segment of a message body: a run of prose, or one attached
 *  file parsed from a ` ```file-upload ` block at the exact position it appeared. */
export type MessageSegment =
  | { type: "text"; text: string }
  | { type: "file"; file: UploadedFile }

/**
 * Split a message body into ordered segments, replacing each ` ```file-upload `
 * block **in place** with a `file` segment so the conversation renderer can draw
 * the attachment chip exactly where the block sat in the markdown — interleaved
 * with the surrounding prose, not hoisted into a separate trailing block.
 *
 * Whitespace-only text runs (the blank lines the composer puts between blocks)
 * are dropped so consecutive attachments don't render with empty paragraphs
 * between them. A message with no blocks yields a single `text` segment (the
 * common case — zero behavioural change for ordinary messages).
 */
export function splitMessageSegments(text: string): MessageSegment[] {
  const out: MessageSegment[] = []
  let last = 0
  let match: RegExpExecArray | null
  BLOCK_RE.lastIndex = 0
  while ((match = BLOCK_RE.exec(text)) !== null) {
    const before = text.slice(last, match.index)
    if (before.trim().length > 0) out.push({ type: "text", text: before })
    const file = parseBlock(match[1])
    if (file) out.push({ type: "file", file })
    last = match.index + match[0].length
  }
  const tail = text.slice(last)
  if (tail.trim().length > 0) out.push({ type: "text", text: tail })
  return out
}

/** Synthesize a {@link FinderNode} for an attached file so it can drive the
 *  shared Quick Look drawer (kind inferred from the filename, like the Finder). */
export function uploadToNode(f: UploadedFile): FinderNode {
  return { name: f.name, path: f.path, kind: kindOf(f.name), size: f.size, modified: "" }
}

/** Human-readable byte size, e.g. `4.2 KB`. */
function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const kb = bytes / 1024
  if (kb < 1024) return `${kb.toFixed(1)} KB`
  return `${(kb / 1024).toFixed(1)} MB`
}

/**
 * Existence-checking wrapper around {@link FileUploadChip}.
 *
 * Resolves whether the referenced file still exists in the agent realm by
 * listing its parent directory (reusing the Finder's `useFs` cache, so an open
 * Finder and a chat chip never double-fetch) and checking for the entry. While
 * the listing loads the chip renders normally (no warning flash); once resolved,
 * a vanished file flips the chip to the greyed "moved / deleted" state (#4).
 *
 * `onOpen` is optional: when provided (the threads chat) the chip is a button
 * that opens the shared Quick Look drawer; when absent (read-only surfaces) it
 * renders as a static chip.
 */
export function MessageFileChip({
  file,
  agentId,
  onOpen,
  onAccent = false,
}: {
  file: UploadedFile
  agentId?: string
  onOpen?: () => void
  /** style for the coloured user bubble (translucent chrome over the accent) */
  onAccent?: boolean
}) {
  const parent = file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/")) : ""
  // No agent → can't verify; assume present rather than cry wolf.
  const { data, loading } = useFs(agentId ?? "", parent)
  const missing =
    !!agentId && !loading && !!data && !data.some((n) => n.path === file.path || n.name === file.name)

  return <FileUploadChip file={file} onOpen={onOpen} missing={missing} onAccent={onAccent} />
}

/**
 * A clickable attachment chip rendered in place of a ` ```file-upload ` block.
 * Shows the file's mac-style icon, name, and size; clicking opens the shared
 * Finder Quick Look drawer ({@link QuickLookSheet}) for the file.
 *
 * Three presentations:
 *   - **present + `onOpen`** → an interactive button (the threads chat).
 *   - **present, no `onOpen`** → a static chip (read-only surfaces).
 *   - **`missing`** → a greyed, non-interactive chip with a warning icon and the
 *     notice "This file does not exist or has been moved elsewhere." (#4).
 */
export function FileUploadChip({
  file,
  onOpen,
  missing = false,
  onAccent = false,
}: {
  file: UploadedFile
  onOpen?: () => void
  missing?: boolean
  onAccent?: boolean
}) {
  // ── Missing: greyed, non-interactive, with an explicit warning. ──
  if (missing) {
    return (
      <span
        className={
          onAccent
            ? "inline-flex max-w-full items-center gap-2 rounded-lg border border-white/25 bg-white/10 px-2.5 py-1.5 text-left text-[12px] opacity-80"
            : "inline-flex max-w-full items-center gap-2 rounded-lg border border-dashed border-border bg-muted/40 px-2.5 py-1.5 text-left text-[12px] opacity-80"
        }
        title="This file does not exist or has been moved elsewhere."
      >
        <AlertTriangle className={onAccent ? "size-3.5 shrink-0" : "size-3.5 shrink-0 text-[var(--warn)]"} />
        <span className="min-w-0 flex-col">
          <span className="block truncate font-medium line-through opacity-90">{file.name}</span>
          <span className="block truncate text-[10.5px] opacity-75">
            This file does not exist or has been moved elsewhere.
          </span>
        </span>
      </span>
    )
  }

  const base =
    "inline-flex max-w-full items-center gap-2 rounded-lg border px-2.5 py-1.5 text-left text-[12px] transition-colors"
  const skin = onAccent
    ? "border-white/25 bg-white/15 hover:bg-white/25"
    : "border-border bg-card card-shadow hover:border-[var(--signal)]/60 hover:bg-muted/40"

  const body = (
    <>
      <span className="shrink-0">
        <FileIcon kind={kindOf(file.name)} ext={extOf(file.name)} size={18} />
      </span>
      <span className={onAccent ? "min-w-0 truncate font-medium" : "min-w-0 truncate font-medium text-foreground/90"}>
        {file.name}
      </span>
      <span className={onAccent ? "shrink-0 text-[10.5px] tabular-nums opacity-75" : "shrink-0 text-[10.5px] tabular-nums text-muted-foreground/70"}>
        {fmtSize(file.size)}
      </span>
      <Paperclip className={onAccent ? "size-3 shrink-0 opacity-60" : "size-3 shrink-0 text-muted-foreground/50"} />
    </>
  )

  // ── Static (no opener) vs. interactive button. ──
  if (!onOpen) {
    return <span className={`${base} ${skin} cursor-default`}>{body}</span>
  }
  return (
    <button onClick={onOpen} className={`${base} ${skin}`}>
      {body}
    </button>
  )
}
