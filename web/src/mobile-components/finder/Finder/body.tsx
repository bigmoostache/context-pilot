import type { FinderNode, FinderViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"
import { ColumnsView, GalleryView, GridView, ListView } from "../views/FinderViews"
import { FinderPreview } from "../preview/FinderPreview"
import { QuickLookSheet } from "../QuickLookSheet"
import type { ViewHandlers } from "../views/helpers"
import type { PinnedFolder } from "../internal/helpers"

/**
 * The shared per-node view props spread onto every Finder view — the views' own
 * {@link ViewHandlers} contract, aliased so the shell has a single import name.
 */
export type ViewProps = ViewHandlers

/**
 * The four view-mode renderers, switched on `viewMode`. Same switch as desktop;
 * on mobile every mode is single-column (grid auto-fills, list/columns are one
 * pane, gallery is hero+filmstrip), so the `overflow` handling below keeps them
 * all vertically scrollable.
 */
function FinderViewSwitch({
  viewMode,
  cwd,
  displayNodes,
  sorted,
  iconSize,
  sortKey,
  asc,
  onSort,
  agentId,
  agentFolder,
  chain,
  previewNode,
  onNavigate,
  viewProps,
}: {
  viewMode: FinderViewMode
  cwd: string
  displayNodes: FinderNode[]
  sorted: FinderNode[]
  iconSize: number
  sortKey: import("@/lib/types").FinderSortKey
  asc: boolean
  onSort: (key: import("@/lib/types").FinderSortKey) => void
  agentId: string
  agentFolder: string
  chain: string[]
  previewNode: FinderNode | null
  onNavigate: (path: string) => void
  viewProps: ViewProps
}) {
  if (viewMode === "grid") {
    return <GridView key={cwd} nodes={displayNodes} iconSize={iconSize} {...viewProps} />
  }
  if (viewMode === "list") {
    return (
      <ListView
        key={cwd}
        nodes={displayNodes}
        sortKey={sortKey}
        asc={asc}
        onSort={onSort}
        {...viewProps}
      />
    )
  }
  if (viewMode === "columns") {
    return (
      <ColumnsView
        agentId={agentId}
        agentFolder={agentFolder}
        chain={chain}
        currentNodes={displayNodes}
        previewNode={previewNode}
        onNavigate={onNavigate}
        {...viewProps}
      />
    )
  }
  return <GalleryView key={cwd} nodes={sorted} hero={previewNode} {...viewProps} />
}

/** Everything {@link FinderBody} needs from the Finder shell. */
export interface FinderBodyProps {
  agentId: string
  agentFolder: string
  root: FinderNode
  cwd: string
  pins: PinnedFolder[]
  fileNode: FinderNode | undefined
  activeTabId: string
  displayNodes: FinderNode[]
  sorted: FinderNode[]
  crumbs: FinderNode[]
  previewNode: FinderNode | null
  viewMode: FinderViewMode
  iconSize: number
  sortKey: import("@/lib/types").FinderSortKey
  asc: boolean
  previewOpen: boolean
  marqueeOn: boolean
  band: { left: number; top: number; width: number; height: number } | null
  mainRef: React.RefObject<HTMLElement | null>
  handlers: React.HTMLAttributes<HTMLDivElement>
  viewProps: ViewProps
  onNavigate: (path: string) => void
  onOpen: (node: FinderNode) => void
  onSort: (key: import("@/lib/types").FinderSortKey) => void
  onEmptyContext: (e: React.MouseEvent) => void
  onCloseTab: (id: string) => void
  onPin: (p: PinnedFolder) => void
  onUnpin: (path: string) => void
  onClearSelection: () => void
  onClosePreview: () => void
}

/**
 * The Finder's main content region — mobile twin. The desktop lays the Favorites
 * sidebar rail beside the view; a phone has no room, so the sidebar is dropped
 * from the body flow entirely (mobile FinderChrome surfaces Favorites via a
 * drawer instead) and the view area is **full-width single-column**. The
 * desktop marquee rubber-band select is gone (no drag-select on touch), so the
 * band overlay + `select-none` guard are omitted. A file tab is still a
 * full-bleed Quick Look of one file. The Quick Look drawer becomes a full-height
 * bottom sheet (mobile QuickLookSheet).
 */
export function FinderBody({
  agentId,
  cwd,
  fileNode,
  activeTabId,
  displayNodes,
  sorted,
  crumbs,
  previewNode,
  viewMode,
  iconSize,
  sortKey,
  asc,
  agentFolder,
  previewOpen,
  marqueeOn,
  mainRef,
  viewProps,
  onNavigate,
  onSort,
  onEmptyContext,
  onCloseTab,
  onClearSelection,
  onClosePreview,
}: FinderBodyProps) {
  return (
    <div className="flex min-h-0 flex-1">
      {fileNode ? (
        <FinderPreview
          node={fileNode}
          variant="full"
          agentId={agentId}
          onClose={() => onCloseTab(activeTabId)}
        />
      ) : (
        <div className="relative flex min-w-0 flex-1">
          <main
            ref={mainRef}
            onContextMenu={onEmptyContext}
            // Empty-space tap clears the selection (no marquee on touch, so the
            // guard is always the active branch). The `marqueeOn ?` ternary shape
            // mirrors the desktop body — it keeps the handler off the statically
            // "always a listener" path that jsx-a11y flags on a <main>, while the
            // keyboard equivalent (Esc → clear) lives on the role="application"
            // shell above.
            onMouseUp={
              marqueeOn
                ? undefined
                : (e) => {
                    if (e.currentTarget !== e.target) return
                    onClearSelection()
                  }
            }
            className={cn(
              "relative min-w-0 flex-1",
              viewMode === "columns" || viewMode === "gallery"
                ? "overflow-hidden"
                : "overflow-auto",
            )}
          >
            <FinderViewSwitch
              viewMode={viewMode}
              cwd={cwd}
              displayNodes={displayNodes}
              sorted={sorted}
              iconSize={iconSize}
              sortKey={sortKey}
              asc={asc}
              onSort={onSort}
              agentId={agentId}
              agentFolder={agentFolder}
              chain={crumbs.map((c) => c.path)}
              previewNode={previewNode}
              onNavigate={onNavigate}
              viewProps={viewProps}
            />
          </main>

          {/* Quick Look — mobile bottom sheet. Only grid & list use it (columns
              shows a compact inline info footer, gallery shows the hero). */}
          <QuickLookSheet
            node={previewNode}
            agentId={agentId}
            open={previewOpen && (viewMode === "grid" || viewMode === "list")}
            onClose={onClosePreview}
          />
        </div>
      )}
    </div>
  )
}
