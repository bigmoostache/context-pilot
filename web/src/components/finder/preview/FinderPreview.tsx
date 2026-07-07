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
 * QuickLook preview pane — the Finder's centerpiece. Renders a rich, kind-aware
 * preview of the selected file: code, markdown, JSON, spreadsheets, slide decks,
 * PDFs, images, audio and video each get a bespoke, beautiful treatment.
 *
 * The pane is a thin shell: {@link Body} routes a node to a bespoke renderer —
 * shared leaf renderers live in `previewParts`, live-data fetchers in
 * `livePreviews`, and the decorative maquette previews in `mockPreviews`.
 */
export function FinderPreview({
  node,
  onClose,
  variant = "pane",
  agentId,
}: {
  node: FinderNode | null
  onClose: () => void
  /** "pane" = the 420px QuickLook side rail; "full" = a file tab's main area */
  variant?: "pane" | "full"
  /** agent realm the file lives in — enables live content fetch for files
   *  whose preview payload isn't inlined (the live Finder). Omit for the mock. */
  agentId?: string | undefined
}) {
  const full = variant === "full"
  return (
    <aside
      className={cn(
        "flex min-h-0 flex-col bg-surface",
        // "full" = a file tab's main area; otherwise the QuickLook pane fills
        // its host. Inside the shadcn Sheet drawer (the only place the pane
        // variant is used) that means it spans the drawer's full width — the
        // Sheet owns the width + left border, so the pane no longer fixes its
        // own 420px or draws a border.
        full ? "min-w-0 flex-1" : "h-full w-full",
      )}
    >
      {!full && (
        <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border px-3">
          <span className="text-[12px] font-semibold text-muted-foreground">Quick Look</span>
          <div className="ml-auto flex items-center gap-1">
            {node && node.kind !== "folder" && (
              <>
                {/* Download: stream the realm file to the browser via the
                    backend's attachment endpoint. Only actionable in the live
                    Finder (an agentId scopes the realm); the mock has none. */}
                <IconBtn
                  icon={Download}
                  title="Download"
                  onClick={agentId ? () => void downloadFile(agentId, node.path) : undefined}
                />
              </>
            )}
            <IconBtn icon={X} title="Close" onClick={onClose} />
          </div>
        </div>
      )}

      {node ? (
        <div key={node.path} className="ql-pop flex min-h-0 flex-1 flex-col">
          <div className="min-h-0 flex-1 overflow-auto">
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
 * content (the decorative Finder mock): code / sheet / slides / pdf / image /
 * media / markdown / text. Returns null when the node has no inline payload, so
 * {@link Body} can fall through to the live-fetch or generic path.
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
 * Finder): images + PDFs stream straight from the backend's inline raw-serve
 * endpoint, text-like kinds fetch their content via the preview/sheet endpoints.
 * Returns null without an agentId (no realm to fetch from) or for an
 * unpreviewable kind, so {@link Body} falls through to {@link Generic}.
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
