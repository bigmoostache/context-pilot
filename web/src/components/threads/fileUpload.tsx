import { Paperclip, AlertTriangle, FolderOpen, Plus, X } from "lucide-react"
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
 * A `/command` offered as a composer suggestion bubble (T348/T350). `command`
 * is the literal slash token (e.g. `/clean`); `name` + `description` label the
 * bubble; `body` is the expanded prompt seeded on click (when present).
 *
 * Lives here — beside {@link UploadedFile} and {@link FileUploadChip} — so the
 * composer's two pill families (file attachments + command suggestions) share
 * ONE module and ONE rendered row ({@link ComposerBubbles}), instead of being
 * two divergent ad-hoc blocks that can't coexist.
 */
export interface CommandSuggestion {
  command: string
  name: string
  description: string
  /** the prompt body the `/command` expands to; seeded into the composer on
   *  click. Falls back to the bare `command` literal when absent. */
  body?: string
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
 *
 * The displayed byte size comes from the **actual file on disk** (the matching
 * listing node's `size`), never from the message's YAML `size:` field — the
 * YAML is author-supplied and can lie, so the chip shows ground truth or
 * nothing. Likewise existence is resolved from the live listing: a file whose
 * parent directory doesn't even exist (the listing fetch 404s) greys out just
 * like one missing from an existing directory.
 */
export function MessageFileChip({
  file,
  agentId,
  onOpen,
  onShowInFinder,
  onAccent = false,
}: {
  file: UploadedFile
  agentId?: string
  onOpen?: () => void
  /** navigate the Finder to this file's parent and select it */
  onShowInFinder?: () => void
  /** style for the coloured user bubble (translucent chrome over the accent) */
  onAccent?: boolean
}) {
  const parent = file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/")) : ""
  // No agent → can't verify; assume present rather than cry wolf.
  const { data, loading, error } = useFs(agentId ?? "", parent)
  // The matching listing node (by full realm path, falling back to basename).
  const node = data?.find((n) => n.path === file.path || n.name === file.name)
  // A 4xx on the parent listing means the directory can't be served — it
  // doesn't exist (a nonexistent parent canonicalizes to nothing → the backend
  // answers 403 "path outside agent realm", NOT 404) or the path escapes the
  // realm. Either way the file is definitively unconfirmable → missing. We key
  // on the client's `"<status> <path>: <body>"` error message (client.ts), so
  // ANY 4xx is caught (the earlier 404-only check missed the 403 that a vanished
  // parent dir actually returns — the "non-existing file shows as present" bug).
  // A statusless error (a real network blip → `TypeError: Failed to fetch`)
  // does NOT match, so a transient fault never greys a genuinely-present file.
  const parentUnservable = /^4\d\d/.test(error?.message ?? "")
  const missing =
    !!agentId && !loading && (parentUnservable || (!!data && !node))
  // Real, on-disk size — the listing node's byte count. `undefined` while the
  // listing loads, when there is no agent to query, or when the file is gone;
  // the chip then omits the size label rather than echo the untrusted YAML.
  const realSize = node?.size

  return (
    <FileUploadChip
      file={file}
      onOpen={onOpen}
      onShowInFinder={onShowInFinder}
      missing={missing}
      onAccent={onAccent}
      size={realSize}
    />
  )
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
  onShowInFinder,
  missing = false,
  onAccent = false,
  size,
}: {
  file: UploadedFile
  onOpen?: () => void
  /** navigate the Finder to this file's parent and select it */
  onShowInFinder?: () => void
  missing?: boolean
  onAccent?: boolean
  /** the file's REAL on-disk byte size (from the listing node), or `undefined`
   *  when unknown — the chip shows the size label only when this is a finite
   *  number, never the untrusted YAML `file.size`. */
  size?: number
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
      {typeof size === "number" && (
        <span className={onAccent ? "shrink-0 text-[10.5px] tabular-nums opacity-75" : "shrink-0 text-[10.5px] tabular-nums text-muted-foreground/70"}>
          {fmtSize(size)}
        </span>
      )}
      <Paperclip className={onAccent ? "size-3 shrink-0 opacity-60" : "size-3 shrink-0 text-muted-foreground/50"} />
    </>
  )

  // ── Static (no opener) vs. interactive button. ──
  if (!onOpen) {
    return (
      <span className="inline-flex items-center gap-1">
        <span className={`${base} ${skin} cursor-default`}>{body}</span>
        {onShowInFinder && !missing && (
          <button
            onClick={onShowInFinder}
            title="Show in Finder"
            className={onAccent
              ? "flex size-6 shrink-0 items-center justify-center rounded-md opacity-60 transition-opacity hover:opacity-100"
              : "flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/50 transition-colors hover:bg-muted/60 hover:text-foreground/80"
            }
          >
            <FolderOpen className="size-3.5" />
          </button>
        )}
      </span>
    )
  }
  return (
    <span className="inline-flex items-center gap-1">
      <button onClick={onOpen} className={`${base} ${skin}`}>
        {body}
      </button>
      {onShowInFinder && !missing && (
        <button
          onClick={onShowInFinder}
          title="Show in Finder"
          className={onAccent
            ? "flex size-6 shrink-0 items-center justify-center rounded-md opacity-60 transition-opacity hover:opacity-100"
            : "flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/50 transition-colors hover:bg-muted/60 hover:text-foreground/80"
          }
        >
          <FolderOpen className="size-3.5" />
        </button>
      )}
    </span>
  )
}

/**
 * The composer's unified bubble row — the SINGLE abstraction shared by the two
 * pill families that sit between the conversation and the textarea: staged
 * file-upload chips and `/command` suggestion bubbles (+ the create-command
 * pill). Both render here, in one flex-wrap row, so they coexist cleanly
 * instead of mutually excluding each other (the pre-fix bug where staging a
 * file hid the slash-command bubbles, and vice-versa).
 *
 * Layout contract (per the T350 request):
 *   - the **container is transparent** (no background) and lives in normal flow,
 *     so the space it occupies is carved out *between* the textarea and the
 *     conversation — it never overlays or hides message content beneath it;
 *   - each **pill is opaque** (`bg-card`), so nothing bleeds through it.
 *
 * Render order: file chips first (most contextual — what you're about to send),
 * then the command suggestions, then the create-command pill. Any subset may be
 * empty; the caller decides whether the row renders at all.
 */
export function ComposerBubbles({
  files = [],
  onRemoveFile,
  suggestions = [],
  onPick,
  onCreateCommand,
}: {
  /** staged-but-unsent uploads, rendered as removable chips */
  files?: UploadedFile[]
  /** remove a staged file by its index in `files` */
  onRemoveFile?: (index: number) => void
  /** `/command` suggestions to offer (empty unless in slash / first-message mode) */
  suggestions?: CommandSuggestion[]
  /** seed the composer from a picked suggestion */
  onPick?: (s: CommandSuggestion) => void
  /** open the create-command dialog (omit to hide the pill) */
  onCreateCommand?: () => void
}) {
  const showCommands = suggestions.length > 0 || !!onCreateCommand
  return (
    <div className="mb-2 flex flex-wrap items-center gap-1.5 bg-transparent">
      {/* Staged file attachments — opaque removable chips. */}
      {files.map((f, i) => (
        <span key={`${f.path}-${i}`} className="inline-flex items-center gap-1">
          <FileUploadChip file={f} size={f.size} />
          <button
            onClick={() => onRemoveFile?.(i)}
            className="flex size-4 items-center justify-center rounded-full bg-muted text-muted-foreground/70 transition-colors hover:bg-destructive/20 hover:text-destructive"
            title="Remove attachment"
          >
            <X className="size-2.5" strokeWidth={3} />
          </button>
        </span>
      ))}

      {/* /command suggestion bubbles — opaque pills. */}
      {showCommands &&
        suggestions.map((s) => (
          <button
            key={s.command}
            type="button"
            onClick={() => onPick?.(s)}
            title={s.description || s.name}
            className="group inline-flex items-center gap-1.5 rounded-full border border-border bg-card px-2.5 py-1 text-[11.5px] text-foreground/75 transition-colors hover:border-[var(--signal)]/60 hover:text-[var(--signal)]"
          >
            <span className="font-mono font-medium text-[var(--interactive)] group-hover:text-[var(--signal)]">
              {s.command}
            </span>
            {s.description && (
              <span className="max-w-[180px] truncate text-muted-foreground/70">{s.description}</span>
            )}
          </button>
        ))}

      {/* Create-command pill — opaque, dashed to read as an action. */}
      {showCommands && onCreateCommand && (
        <button
          type="button"
          onClick={onCreateCommand}
          title="Create a new /command"
          className="group inline-flex items-center gap-1 rounded-full border border-dashed border-border bg-card px-2.5 py-1 text-[11.5px] text-muted-foreground/80 transition-colors hover:border-[var(--signal)]/60 hover:text-[var(--signal)]"
        >
          <Plus className="size-3 text-muted-foreground/70 group-hover:text-[var(--signal)]" strokeWidth={2.5} />
          <span className="font-medium">create command</span>
        </button>
      )}
    </div>
  )
}
