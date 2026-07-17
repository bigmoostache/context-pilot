import { AlertTriangle, FolderOpen, Plus, X } from "lucide-react"
import { kindOf, extOf } from "@/components/finder/support/kind"
import { FileIcon } from "@/components/finder/support/macIcons"
import { useFs } from "@/lib/live"
import type { CommandSuggestion, UploadedFile } from "./helpers"

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
  agentId?: string | undefined
  onOpen?: (() => void) | undefined
  /** navigate the Finder to this file's parent and select it */
  onShowInFinder?: (() => void) | undefined
  /** style for the coloured user bubble (translucent chrome over the accent) */
  onAccent?: boolean | undefined
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
  const missing = !!agentId && !loading && (parentUnservable || (!!data && !node))
  // Real, on-disk size — the listing node's byte count. `undefined` while the
  // listing loads, when there is no agent to query, or when the file is gone;
  // the chip then omits the size label rather than echo the untrusted YAML.
  const realSize = node?.size ?? undefined

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
 * A clickable attachment card rendered in place of a ` ```file-upload ` block.
 * A compact two-line card: a mac-style icon in a rounded well, the file name,
 * and its on-disk size; clicking opens the shared Finder Quick Look drawer
 * (`QuickLookSheet`) — the whole card is the click target.
 *
 * Three presentations:
 *   - **present + `onOpen`** → an interactive button (chat).
 *   - **present, no `onOpen`** → a static card (read-only surfaces).
 *   - **`missing`** → a greyed, dashed, non-interactive card with a warning icon
 *     and the notice "This file does not exist or has been moved elsewhere." (#4).
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
  onOpen?: (() => void) | undefined
  /** navigate the Finder to this file's parent and select it */
  onShowInFinder?: (() => void) | undefined
  missing?: boolean | undefined
  onAccent?: boolean | undefined
  /** the file's REAL on-disk byte size (from the listing node), or `undefined`
   *  when unknown — the card shows the size label only when this is a finite
   *  number, never the untrusted YAML `file.size`. */
  size?: number | undefined
}) {
  // ── Missing: greyed, dashed, non-interactive, with an explicit warning. ──
  if (missing) return <MissingChip file={file} onAccent={onAccent} />

  const base =
    "group/chip inline-flex max-w-full items-center gap-2.5 rounded-xl border px-2.5 py-1.5 text-left align-middle transition-[border-color,background-color,transform,box-shadow]"
  const skin = onAccent
    ? "border-white/25 bg-white/12 hover:bg-white/20 hover:-translate-y-px"
    : "border-border/70 bg-card card-shadow hover:border-(--signal)/55 hover:bg-muted/30 hover:-translate-y-px"

  const body = <FileChipBody file={file} size={size} onAccent={onAccent} />
  // Static (no opener) vs. interactive trigger — same content, one wrapper each.
  // Both carry no glyph; the whole card is the click target when interactive.
  const trigger = onOpen ? (
    <button onClick={onOpen} className={`${base} ${skin}`}>
      {body}
    </button>
  ) : (
    <span className={`${base} ${skin} cursor-default`}>{body}</span>
  )
  return (
    <span className="inline-flex items-center gap-1 align-middle">
      {trigger}
      <ShowInFinderButton onShowInFinder={onShowInFinder} onAccent={onAccent} />
    </span>
  )
}

/** The greyed dashed card shown when the referenced file is gone (#4). Split out
 *  so {@link FileUploadChip}'s live path stays flat against the P8 complexity
 *  budget. */
function MissingChip({ file, onAccent }: { file: UploadedFile; onAccent: boolean }) {
  return (
    <span
      className={
        onAccent
          ? "inline-flex max-w-full items-center gap-2.5 rounded-xl border border-dashed border-white/25 bg-white/8 px-2.5 py-1.5 text-left align-middle opacity-80"
          : "inline-flex max-w-full items-center gap-2.5 rounded-xl border border-dashed border-border bg-muted/30 px-2.5 py-1.5 text-left align-middle opacity-80"
      }
      title="This file does not exist or has been moved elsewhere."
    >
      <span
        className={
          onAccent
            ? "flex size-7 shrink-0 items-center justify-center rounded-lg bg-white/10"
            : "flex size-7 shrink-0 items-center justify-center rounded-lg bg-muted/60"
        }
      >
        <AlertTriangle
          className={onAccent ? "size-3.5 shrink-0" : "size-3.5 shrink-0 text-(--warn)"}
        />
      </span>
      <span className="flex min-w-0 flex-col">
        <span className="truncate text-[12px] font-medium line-through opacity-90">{file.name}</span>
        <span className="truncate text-[10px] opacity-75">moved or deleted — no longer here</span>
      </span>
    </span>
  )
}

/** The "Show in Finder" affordance beside a card — renders nothing when no
 *  navigation handler is supplied. Extracted so {@link FileUploadChip} isn't
 *  duplicated across the static/interactive tails (each copy cost the card two
 *  branches toward the P8 complexity budget). */
function ShowInFinderButton({
  onShowInFinder,
  onAccent,
}: {
  onShowInFinder?: (() => void) | undefined
  onAccent: boolean
}) {
  if (!onShowInFinder) return null
  return (
    <button
      onClick={onShowInFinder}
      title="Show in Finder"
      className={
        onAccent
          ? "flex size-7 shrink-0 items-center justify-center rounded-lg opacity-55 transition-opacity hover:opacity-100"
          : "flex size-7 shrink-0 items-center justify-center rounded-lg text-muted-foreground/45 transition-colors hover:bg-muted/60 hover:text-foreground/80"
      }
    >
      <FolderOpen className="size-3.5" />
    </button>
  )
}

/** A card's inner content (icon well · name · size), shared by the static and
 *  interactive triggers. The mac-style file icon sits in a rounded well and the
 *  name/size stack in a tight two-line column. */
function FileChipBody({
  file,
  size,
  onAccent,
}: {
  file: UploadedFile
  size?: number | undefined
  onAccent: boolean
}) {
  return (
    <>
      <span
        className={
          onAccent
            ? "flex size-7 shrink-0 items-center justify-center rounded-lg bg-white/15"
            : "flex size-7 shrink-0 items-center justify-center rounded-lg bg-muted/50"
        }
      >
        <FileIcon kind={kindOf(file.name)} ext={extOf(file.name)} size={18} />
      </span>
      <span className="flex min-w-0 flex-col leading-tight">
        <span
          className={
            onAccent
              ? "truncate text-[12px] font-medium"
              : "truncate text-[12px] font-medium text-foreground/90"
          }
        >
          {file.name}
        </span>
        {typeof size === "number" && (
          <span
            className={
              onAccent
                ? "truncate text-[10px] tabular-nums opacity-70"
                : "truncate text-[10px] text-muted-foreground/60 tabular-nums"
            }
          >
            {fmtSize(size)}
          </span>
        )}
      </span>
    </>
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
  files?: UploadedFile[] | undefined
  /** remove a staged file by its index in `files` */
  onRemoveFile?: ((index: number) => void) | undefined
  /** `/command` suggestions to offer (empty unless in slash / first-message mode) */
  suggestions?: CommandSuggestion[] | undefined
  /** seed the composer from a picked suggestion */
  onPick?: ((s: CommandSuggestion) => void) | undefined
  /** open the create-command dialog (omit to hide the pill) */
  onCreateCommand?: (() => void) | undefined
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
            className="group inline-flex items-center gap-1.5 rounded-full border border-border bg-card px-2.5 py-1 text-[11.5px] text-foreground/75 transition-colors hover:border-(--signal)/60 hover:text-(--signal)"
          >
            <span className="font-mono font-medium text-(--interactive) group-hover:text-(--signal)">
              {s.command}
            </span>
            {s.description && (
              <span className="max-w-[180px] truncate text-muted-foreground/70">
                {s.description}
              </span>
            )}
          </button>
        ))}

      {/* Create-command pill — opaque, dashed to read as an action. */}
      {showCommands && onCreateCommand && (
        <button
          type="button"
          onClick={onCreateCommand}
          title="Create a new /command"
          className="group inline-flex items-center gap-1 rounded-full border border-dashed border-border bg-card px-2.5 py-1 text-[11.5px] text-muted-foreground/80 transition-colors hover:border-(--signal)/60 hover:text-(--signal)"
        >
          <Plus
            className="size-3 text-muted-foreground/70 group-hover:text-(--signal)"
            strokeWidth={2.5}
          />
          <span className="font-medium">create command</span>
        </button>
      )}
    </div>
  )
}
