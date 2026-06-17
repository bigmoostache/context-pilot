import { useEffect } from "react"
import { X } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { childCounts, fmtBytes, nodeSize } from "@/lib/finderFs"
import { extOf, kindMeta, TAG_META } from "./kind"
import { FileIcon } from "./macIcons"

/**
 * "Get Info" inspector — a macOS-style modal sheet with the full dossier for a
 * node: large kind icon, name, kind/size/dates, tags, dimensions/duration, and
 * a "where" path. Decorative but complete.
 */
export function GetInfo({ node, onClose }: { node: FinderNode; onClose: () => void }) {
  useEffect(() => {
    const onEsc = (e: KeyboardEvent) => e.key === "Escape" && onClose()
    window.addEventListener("keydown", onEsc)
    return () => window.removeEventListener("keydown", onEsc)
  }, [onClose])

  const M = kindMeta[node.kind]
  const isFolder = node.kind === "folder"
  const counts = childCounts(node)

  return (
    <div
      className="absolute inset-0 z-40 flex items-center justify-center bg-black/30 backdrop-blur-[2px]"
      onClick={onClose}
    >
      <div
        className="ql-pop flex w-[340px] flex-col rounded-2xl border border-border bg-popover/95 backdrop-blur-xl pop-shadow"
        onClick={(e) => e.stopPropagation()}
      >
        {/* header */}
        <div className="flex items-center gap-2 border-b border-border/70 px-3 py-2">
          <span className="text-[12px] font-semibold text-foreground/80">
            {node.name} Info
          </span>
          <button
            onClick={onClose}
            className="ml-auto flex size-6 items-center justify-center rounded-md text-muted-foreground/60 hover:bg-muted/60 hover:text-foreground"
          >
            <X className="size-3.5" />
          </button>
        </div>

        {/* identity */}
        <div className="flex items-center gap-3.5 px-5 py-4">
          <FileIcon kind={node.kind} ext={extOf(node.name)} size={64} className="shrink-0" />
          <div className="flex min-w-0 flex-col gap-0.5">
            <span className="truncate text-[15px] font-semibold text-foreground">{node.name}</span>
            <span className="text-[12px] tabular-nums text-muted-foreground">
              {fmtBytes(nodeSize(node))}
            </span>
            {node.tags && node.tags.length > 0 && (
              <div className="mt-1 flex items-center gap-1">
                {node.tags.map((t) => (
                  <span
                    key={t}
                    title={TAG_META[t].label}
                    className="size-2.5 rounded-full ring-1 ring-inset ring-black/10"
                    style={{ background: TAG_META[t].color }}
                  />
                ))}
              </div>
            )}
          </div>
        </div>

        <div className="h-px bg-border/60" />

        {/* dossier */}
        <dl className="grid grid-cols-[88px_1fr] gap-x-3 gap-y-2 px-5 py-4 text-[12px]">
          <Row k="Kind" v={M.label} />
          {isFolder ? (
            <Row k="Contains" v={`${counts.folders} folders, ${counts.files} files`} />
          ) : (
            <Row k="Size" v={fmtBytes(node.size)} />
          )}
          {node.image && <Row k="Dimensions" v={`${node.image.w} × ${node.image.h}`} />}
          {node.media && <Row k="Duration" v={node.media.duration} />}
          {node.pdf && <Row k="Pages" v={`${node.pdf.pages}`} />}
          <Row k="Created" v={node.created ?? "—"} />
          <Row k="Modified" v={node.modified} />
          <Row k="Where" v={node.path} mono />
        </dl>

        <div className="flex items-center justify-between border-t border-border/70 px-5 py-3">
          <span className="text-[11px] text-muted-foreground/60">Get Info</span>
          <button
            onClick={onClose}
            className="rounded-lg bg-[var(--signal)] px-3.5 py-1.5 text-[12px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
          >
            Done
          </button>
        </div>
      </div>
    </div>
  )
}

function Row({ k, v, mono }: { k: string; v: string; mono?: boolean }) {
  return (
    <>
      <dt className="text-right text-muted-foreground">{k}</dt>
      <dd className={"min-w-0 break-words text-foreground/85 " + (mono ? "font-mono text-[10.5px]" : "")}>
        {v}
      </dd>
    </>
  )
}
