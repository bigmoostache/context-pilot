import { X } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { Markdown } from "@/lib/support/markdown"
import { extOf } from "../support/kind"
import { FileIcon } from "../support/macIcons"
import { cn } from "@/lib/utils"

/** A small icon-only button used in the Quick Look pane header. */
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
      onClick={onClick}
      className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/70 hover:text-foreground"
    >
      <Icon className="size-3.5" />
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
export function MarkdownPreview({ text, truncated }: { text: string; truncated?: boolean }) {
  return (
    <div className="bg-card p-4">
      <Markdown text={text} className="text-[12.5px] text-foreground/85" />
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
          "text-[11.5px] leading-relaxed wrap-break-word whitespace-pre-wrap text-foreground/85",
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
        <span className="text-[14px] font-semibold text-foreground/90">{node.name}</span>
        <span className="text-[12px] text-muted-foreground">
          {folders} folder{folders === 1 ? "" : "s"} · {files} file{files === 1 ? "" : "s"}
        </span>
      </div>
      {/* mini contents stack */}
      <div className="mt-1 flex w-full flex-col gap-1 px-2">
        {kids.slice(0, 5).map((k) => (
          <div
            key={k.path}
            className="flex items-center gap-2 rounded-md bg-muted/40 px-2 py-1 text-left text-[11px]"
          >
            <FileIcon kind={k.kind} ext={extOf(k.name)} size={15} className="shrink-0" />
            <span className="truncate text-foreground/75">{k.name}</span>
          </div>
        ))}
        {kids.length > 5 && (
          <span className="text-[10.5px] text-muted-foreground/60">+{kids.length - 5} more</span>
        )}
      </div>
    </div>
  )
}

export function Generic({ node }: { node: FinderNode }) {
  return (
    <div className="flex flex-col items-center gap-3 py-8 text-center">
      <FileIcon kind={node.kind} ext={extOf(node.name)} size={68} />
      <span className="text-[13px] text-muted-foreground">No preview available</span>
    </div>
  )
}

export function Empty() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
      <span className="text-[12.5px] text-muted-foreground/60">
        Select a file to preview it here.
      </span>
      <span className="text-[11px] text-muted-foreground/40">Press Space for Quick Look</span>
    </div>
  )
}
