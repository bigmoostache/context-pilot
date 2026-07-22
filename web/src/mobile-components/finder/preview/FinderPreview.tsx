import type { ReactElement } from "react"
import { Download, X } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { downloadFile } from "@/lib/api"
import { cn } from "@/lib/utils"
import {
  Empty,
  FolderPreview,
  Generic,
  IconBtn,
  MarkdownPreview,
  TextPreview,
} from "./previewParts"
import { LiveImagePreview, LivePdfPreview, LivePreview, LiveSheetPreview } from "./livePreviews"
import { TEXT_KINDS } from "./kinds"
import {
  AudioPreview,
  CodePreview,
  ImagePreview,
  PdfPreview,
  SheetPreview,
  SlidesPreview,
  VideoPreview,
} from "./mockPreviews"

/**
 * Quick Look preview pane — mobile twin. On the phone the "pane" variant is
 * hosted inside the Quick Look bottom sheet (mobile QuickLookSheet), so it fills
 * the sheet's full width; the "full" variant is a file tab's main area. The only
 * fork from desktop is touch-sized header controls (the shared {@link IconBtn} is
 * a 40px active-press button) and a slightly taller header bar. The kind routing
 * ({@link Body}) is byte-identical.
 */
export function FinderPreview({
  node,
  onClose,
  variant = "pane",
  agentId,
}: {
  node: FinderNode | null
  onClose: () => void
  /** "pane" = the Quick Look sheet body; "full" = a file tab's main area */
  variant?: "pane" | "full"
  /** agent realm the file lives in — enables live content fetch for files
   *  whose preview payload isn't inlined (the live Finder). Omit for the mock. */
  agentId?: string | undefined
}) {
  const full = variant === "full"
  return (
    <aside
      className={cn("flex min-h-0 flex-col bg-surface", full ? "min-w-0 flex-1" : "size-full")}
    >
      {!full && (
        <div className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-3">
          <span className="text-[13px] font-semibold text-muted-foreground">Quick Look</span>
          <div className="ml-auto flex items-center gap-1">
            {node && node.kind !== "folder" && (
              <IconBtn
                icon={Download}
                title="Download"
                onClick={agentId ? () => void downloadFile(agentId, node.path) : undefined}
              />
            )}
            <IconBtn icon={X} title="Close" onClick={onClose} />
          </div>
        </div>
      )}

      {node ? (
        <div key={node.path} className="ql-pop flex min-h-0 flex-1 flex-col">
          <div className="flex min-h-0 flex-1 flex-col overflow-auto">
            <Body node={node} agentId={agentId} />
          </div>
        </div>
      ) : (
        <Empty />
      )}
    </aside>
  )
}

/**
 * Render an inlined-payload preview for a node that carries its own maquette
 * content (the decorative Finder mock). Same routing as desktop; the mock
 * renderers are shared (kept as @generated stubs re-exporting desktop — they're
 * decorative, no touch divergence).
 */
function inlinePreview(node: FinderNode): ReactElement | null {
  if (node.kind === "folder") return <FolderPreview node={node} />
  if (node.code) return <CodePreview lang={node.code.lang} lines={node.code.lines} />
  if (node.sheet) return <SheetPreview sheet={node.sheet} />
  if (node.slides) return <SlidesPreview slides={node.slides} />
  if (node.pdf) return <PdfPreview pdf={node.pdf} />
  if (node.image) return <ImagePreview image={node.image} />
  if (node.media?.kind === "audio") return <AudioPreview media={node.media} />
  if (node.media?.kind === "video") return <VideoPreview media={node.media} />
  if (node.kind === "markdown" && node.text) return <MarkdownPreview text={node.text} />
  if (node.text) return <TextPreview kind={node.kind} text={node.text} />
  return null
}

/**
 * Render a live-fetched preview for a node whose payload isn't inlined (the live
 * Finder), through the mobile live renderers (touch zoom / copy / save). Returns
 * null without an agentId or for an unpreviewable kind so {@link Body} falls
 * through to {@link Generic}.
 */
function livePreview(node: FinderNode, agentId: string | undefined): ReactElement | null {
  if (!agentId) return null
  if (node.kind === "image") return <LiveImagePreview agentId={agentId} node={node} />
  if (node.kind === "pdf") return <LivePdfPreview agentId={agentId} node={node} />
  if (node.kind === "sheet") return <LiveSheetPreview agentId={agentId} node={node} />
  if (TEXT_KINDS.has(node.kind)) return <LivePreview agentId={agentId} node={node} />
  return null
}

/**
 * Route a node to its bespoke preview: an inlined maquette payload first, then a
 * live backend fetch (given an agentId), then the generic no-preview fallback.
 */
function Body({ node, agentId }: { node: FinderNode; agentId?: string | undefined }) {
  return inlinePreview(node) ?? livePreview(node, agentId) ?? <Generic node={node} />
}
