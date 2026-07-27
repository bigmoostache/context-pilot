import { ChevronRight } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { fmtBytes, fmtModified, sortNodes } from "@/lib/support/finderFs"
import { extOf, kindMeta } from "../support/kind"
import { FileIcon } from "../support/macIcons"
import { InfoBadge } from "../support/InfoBadge"
import { cn } from "@/lib/utils"
import { mods, startItemDrag, type ViewHandlers } from "./helpers"
import { RenameInput, TagDots } from "./shared"

/**
 * Column browser — mobile twin of `components/finder/views/ColumnsView`.
 *
 * The desktop is a horizontal Miller-columns stack (one column per ancestor in
 * the path chain, side by side, `overflow-x-auto`). A phone has no width for
 * several columns abreast, so the column view **collapses to a single full-width
 * pane** showing only the current (deepest) folder — exactly how the iOS Files
 * app degrades "columns" on iPhone: it shows one level and you drill in. Tapping
 * a folder navigates into it (the shared `onNavigate` grows the chain); the
 * breadcrumb bar in the mobile FinderChrome walks back up. A selected file shows
 * a compact inline info footer instead of the desktop trailing preview pane
 * (Quick Look owns full preview on mobile).
 *
 * `chain` is accepted for path-parity with the desktop signature but unused —
 * the single pane only ever lists the current dir's `currentNodes`.
 */
export function ColumnsView({
  currentNodes,
  previewNode,
  onNavigate,
  ...h
}: ViewHandlers & {
  agentId: string
  agentFolder: string
  chain: string[]
  currentNodes: FinderNode[]
  previewNode: FinderNode | null
  onNavigate: (path: string) => void
}) {
  const nodes = sortNodes(currentNodes, "name", true)
  const showInfo = previewNode && previewNode.kind !== "folder"

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1 overflow-y-auto py-1">
        {nodes.map((n) => {
          const sel = h.selected.has(n.path)
          return (
            <button
              key={n.path}
              draggable={h.renamingPath !== n.path}
              onDragStart={(e) => startItemDrag(e, n, h.selected)}
              onClick={(e) => {
                // Single tap: folders drill in (grow the chain), files select
                // (drives Quick Look). No hover-preview / double-click on touch.
                if (n.kind === "folder") onNavigate(n.path)
                else h.onClick(n, mods(e))
              }}
              onContextMenu={(e) => h.onContext(e, n)}
              className={cn(
                "mx-1 flex items-center gap-3 rounded-md p-2.5 text-left text-[13.5px] transition-colors select-none",
                sel
                  ? "bg-(--signal)/20 font-medium text-foreground"
                  : "text-foreground/80 active:bg-muted/45",
              )}
            >
              <FileIcon kind={n.kind} ext={extOf(n.name)} size={22} className="shrink-0" />
              {h.renamingPath === n.path && h.onRenameCommit && h.onRenameCancel ? (
                <RenameInput
                  node={n}
                  onCommit={(name) => h.onRenameCommit?.(n, name)}
                  onCancel={() => h.onRenameCancel?.()}
                />
              ) : (
                <span className="min-w-0 flex-1 truncate font-medium">{n.name}</span>
              )}
              <TagDots tags={n.tags} />
              {h.descriptions?.[n.path] && <InfoBadge description={h.descriptions[n.path]} />}
              {n.kind === "folder" && (
                <ChevronRight className="size-4 shrink-0 text-muted-foreground/50" />
              )}
            </button>
          )
        })}
      </div>

      {showInfo && (
        <div className="flex shrink-0 items-center gap-3 border-t border-border bg-surface/60 px-4 py-3">
          <FileIcon kind={previewNode.kind} ext={extOf(previewNode.name)} size={34} />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-[13px] font-semibold text-foreground/90">
              {previewNode.name}
            </span>
            <span className="truncate text-[11px] text-muted-foreground">
              {kindMeta[previewNode.kind].label} · {fmtBytes(previewNode.size)} ·{" "}
              {fmtModified(previewNode.modified)}
            </span>
          </div>
          <TagDots tags={previewNode.tags} />
        </div>
      )}
    </div>
  )
}
