import type { FinderNode, FinderViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"
import { FinderSidebar } from "../FinderChrome"
import { ColumnsView, GalleryView, GridView, ListView } from "../views/FinderViews"
import { FinderPreview } from "../preview/FinderPreview"
import { QuickLookSheet } from "../QuickLookSheet"
import type { ViewHandlers } from "../views/helpers"
import type { PinnedFolder } from "../internal/helpers"

/**
 * The shared per-node view props (selection + interaction handlers) spread onto
 * every Finder view. This is exactly the views' own {@link ViewHandlers}
 * contract — aliased here so the Finder shell has a single import name for the
 * bag it threads from {@link useFinderController} through {@link FinderBody} into
 * the four view components (whose props are `ViewHandlers & {…}`).
 */
export type ViewProps = ViewHandlers

/**
 * The four view-mode renderers, switched on `viewMode`. Grid / list / gallery
 * key on `cwd` so switching directories remounts (fresh scroll + selection
 * paint); columns is a Miller stack that navigates in place. Extracted from
 * {@link FinderBody} so each function stays within the P8 complexity budget.
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
 * The Finder's main content region: the Favorites sidebar (folder tabs only),
 * the scrollable view area with its four view modes + marquee band, and the
 * grid/list Quick Look drawer — OR a full-bleed single-file preview when the
 * active tab is a file. Extracted from {@link Finder} so the shell's render
 * stays within the P8 line/complexity budgets.
 */
export function FinderBody({
  agentId,
  agentFolder,
  root,
  cwd,
  pins,
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
  previewOpen,
  marqueeOn,
  band,
  mainRef,
  handlers,
  viewProps,
  onNavigate,
  onOpen,
  onSort,
  onEmptyContext,
  onCloseTab,
  onPin,
  onUnpin,
  onClearSelection,
  onClosePreview,
}: FinderBodyProps) {
  return (
    <div className="flex min-h-0 flex-1">
      {/* The Favorites/Locations/Tags sidebar belongs to folder browsing. On a
          file tab the main area is a full-bleed QuickLook of one file, so the
          explorer sidebar is irrelevant — hide it entirely. */}
      {!fileNode && (
        <FinderSidebar
          root={root}
          cwd={cwd}
          pins={pins}
          onNavigate={onNavigate}
          onOpen={onOpen}
          onPin={onPin}
          onUnpin={onUnpin}
        />
      )}

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
            {...handlers}
            onContextMenu={onEmptyContext}
            // Empty-space click clears the selection. In grid/list the marquee
            // hook already fires this on a no-drag mouse-up (its onEmptyClick);
            // columns/gallery disable the marquee, so bind a plain mouse-up
            // there. Using onMouseUp (not onClick) matches the marquee's own
            // pointer handlers and stays clear of the jsx-a11y click-handler
            // rules — a background scroll surface must not become a tab stop,
            // and the keyboard equivalent (Esc → clear) already lives on the
            // role="application" surface above.
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
              marqueeOn && "select-none",
            )}
          >
            {band && (
              <div
                className="pointer-events-none absolute z-10 rounded-[2px] border border-[var(--signal)] bg-[var(--signal)]/12"
                style={{ left: band.left, top: band.top, width: band.width, height: band.height }}
              />
            )}
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

          {/* Quick Look — a standard shadcn Sheet drawer from the right edge.
              It is a normal MODAL sheet: the component brings the dimming
              backdrop, slide-in/out animation, focus trap, scroll lock, and
              Esc + click-outside dismissal for free. Only grid & list use it —
              columns has its own trailing Miller preview pane and gallery shows
              the selected item as a hero, so a drawer there would double up.
              The Sheet's built-in close button is hidden because the pane
              renders its own Quick Look header with a Close control. */}
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
