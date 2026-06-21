import { Paperclip } from "lucide-react"
import { kindOf, extOf } from "@/components/finder/support/kind"
import { FileIcon } from "@/components/finder/support/macIcons"
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
 * uploaded file. The conversation renderer ({@link parseFileUploads}) extracts
 * these blocks and turns them into clickable preview chips; the agent reads the
 * same YAML as plain context, so it knows which files were attached.
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

/**
 * Split a message body into its prose (with every `file-upload` block removed)
 * and the list of attached files parsed from those blocks. A message that is
 * purely attachments yields an empty `clean` string and one entry per block.
 * Tolerant: a block missing `name` falls back to the path's basename, a missing
 * `size` to 0.
 */
export function parseFileUploads(text: string): { clean: string; files: UploadedFile[] } {
  const files: UploadedFile[] = []
  let match: RegExpExecArray | null
  BLOCK_RE.lastIndex = 0
  while ((match = BLOCK_RE.exec(text)) !== null) {
    const body = match[1]
    const path = field(body, "path")
    if (!path) continue
    files.push({
      path,
      name: field(body, "name") || path.split("/").pop() || path,
      size: Number(field(body, "size")) || 0,
      note: field(body, "note"),
    })
  }
  const clean = text.replace(BLOCK_RE, "").trim()
  return { clean, files }
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
 * A clickable attachment chip rendered in place of a `file-upload` block. Shows
 * the file's mac-style icon, name, and size; clicking opens the shared Finder
 * Quick Look drawer ({@link QuickLookSheet}) for the file.
 */
export function FileUploadChip({ file, onOpen }: { file: UploadedFile; onOpen: () => void }) {
  return (
    <button
      onClick={onOpen}
      className="inline-flex max-w-full items-center gap-2 rounded-lg border border-border bg-card px-2.5 py-1.5 text-left text-[12px] transition-colors card-shadow hover:border-[var(--signal)]/60 hover:bg-muted/40"
    >
      <span className="shrink-0">
        <FileIcon kind={kindOf(file.name)} ext={extOf(file.name)} size={18} />
      </span>
      <span className="min-w-0 truncate font-medium text-foreground/90">{file.name}</span>
      <span className="shrink-0 text-[10.5px] tabular-nums text-muted-foreground/70">
        {fmtSize(file.size)}
      </span>
      <Paperclip className="size-3 shrink-0 text-muted-foreground/50" />
    </button>
  )
}
