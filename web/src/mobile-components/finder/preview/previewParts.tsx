import { X } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { Markdown } from "@/lib/support/markdown"
import { extOf } from "../support/kind"
import { FileIcon } from "../support/macIcons"
import { cn } from "@/lib/utils"

/**
 * Icon-only button in the mobile Quick Look header — mobile twin of the desktop
 * previewParts IconBtn. The desktop is a 28px (size-7) hover-highlighted button;
 * a phone has no hover and 28px is below a comfortable thumb target, so this is
 * a 40px (size-10) button with an `active:` press state instead of `hover:`.
 */
export function IconBtn({
  icon: Icon,
  title,
  onClick,
}: {
  icon: typeof X
  title: string
  onClick?: (() => void) | undefined
}) {
  return (
    <button
      title={title}
      aria-label={title}
      onClick={onClick}
      className="flex size-10 items-center justify-center rounded-md text-muted-foreground/70 transition-colors active:bg-muted/70 active:text-foreground"
    >
      <Icon className="size-4" />
    </button>
  )
}

/** A centered, muted status line shown while a live preview loads. */
export function PreviewStatus({ label }: { label: string }) {
  return (
    <div className="flex flex-1 items-center justify-center py-12">
      <span className="text-[12.5px] text-muted-foreground/70">{label}</span>
    </div>
  )
}

/** A subtle footer noting the backend capped the preview at 256 KiB. */
export function TruncatedNote() {
  return (
    <p className="mt-3 border-t border-border pt-2 text-[10.5px] text-muted-foreground/60 italic">
      Preview truncated — file exceeds 256 KiB.
    </p>
  )
}

// ── markdown (rendered) ───────────────────────────────────────────
// Rendered markdown reads identically on touch — no fork beyond the shared
// mobile tree membership; kept a real twin for path parity + leak-clean imports.
export function MarkdownPreview({ text, truncated }: { text: string; truncated?: boolean }) {
  return (
    <div className="bg-card p-4">
      <Markdown text={text} className="text-[13px] text-foreground/85" />
      {truncated && <TruncatedNote />}
    </div>
  )
}

// ── text / json ───────────────────────────────────────────────────
export function TextPreview({
  kind,
  text,
  truncated,
}: {
  kind: FinderNode["kind"]
  text: string
  truncated?: boolean
}) {
  const mono = kind === "json" || kind === "code"
  return (
    <div className="bg-card p-3.5">
      <pre
        className={cn(
          // 12.5px (vs desktop 11.5) — a touch more legible on a small screen.
          "text-[12.5px] leading-relaxed wrap-break-word whitespace-pre-wrap text-foreground/85",
          mono ? "font-mono" : "font-sans",
        )}
      >
        {text}
      </pre>
      {truncated && <TruncatedNote />}
    </div>
  )
}

// ── folder ────────────────────────────────────────────────────────
export function FolderPreview({ node }: { node: FinderNode }) {
  const kids = node.children ?? []
  const folders = kids.filter((k) => k.kind === "folder").length
  const files = kids.length - folders
  return (
    <div className="flex flex-col items-center gap-3 py-6 text-center">
      <FileIcon kind="folder" size={68} />
      <div className="flex flex-col gap-0.5">
        <span className="text-[15px] font-semibold text-foreground/90">{node.name}</span>
        <span className="text-[13px] text-muted-foreground">
          {folders} folder{folders === 1 ? "" : "s"} · {files} file{files === 1 ? "" : "s"}
        </span>
      </div>
      {/* mini contents stack — rows are py-2 for a comfortable tap-height feel */}
      <div className="mt-1 flex w-full flex-col gap-1 px-2">
        {kids.slice(0, 5).map((k) => (
          <div
            key={k.path}
            className="flex items-center gap-2 rounded-md bg-muted/40 p-2 text-left text-[12px]"
          >
            <FileIcon kind={k.kind} ext={extOf(k.name)} size={16} className="shrink-0" />
            <span className="truncate text-foreground/75">{k.name}</span>
          </div>
        ))}
        {kids.length > 5 && (
          <span className="text-[11px] text-muted-foreground/60">+{kids.length - 5} more</span>
        )}
      </div>
    </div>
  )
}

export function Generic({ node }: { node: FinderNode }) {
  return (
    <div className="flex flex-col items-center gap-3 py-8 text-center">
      <FileIcon kind={node.kind} ext={extOf(node.name)} size={68} />
      <span className="text-[14px] text-muted-foreground">No preview available</span>
    </div>
  )
}

export function Empty() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
      <span className="text-[13px] text-muted-foreground/60">Tap a file to preview it here.</span>
    </div>
  )
}
