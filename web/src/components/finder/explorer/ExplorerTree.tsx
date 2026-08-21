import { useFs } from "@/lib/live"
import type { FinderNode } from "@/lib/types"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Loader2 } from "lucide-react"
import { ExplorerRow } from "./ExplorerRow"
import { relOf, sortTreeNodes, type TreeState } from "./treeState"

/**
 * The VS Code explorer — a nested, lazily-fetched file tree.
 *
 * THE TREE FETCHES PER OPEN FOLDER, and that is the whole architecture. There
 * is no single "give me the realm" call and no client-side copy of the
 * filesystem: {@link ExplorerChildren} is MOUNTED only while its folder is
 * expanded, so its `useFs` call is made when the folder opens and torn down
 * when it closes. Consequences worth stating, because they are the reason for
 * the shape rather than side effects of it:
 *
 *   * a realm with ten thousand files costs exactly as much as the folders the
 *     user actually opened;
 *   * a collapsed folder cannot show stale children, because it holds none;
 *   * re-opening a folder re-reads it, so a file the agent wrote while the
 *     folder was shut is simply there.
 *
 * Conditional hooks are illegal, which is why "fetch only when open" is
 * expressed as MOUNTING a child component rather than as an `enabled` flag —
 * the flag would leave a live query per folder the user ever touched.
 */
export function ExplorerTree({
  agentId,
  agentFolder,
  tree,
  descriptions,
  activePath,
  onOpenFile,
  onPinFile,
  onContext,
}: {
  agentId: string
  /** Absolute realm root. Every path below is derived against it. */
  agentFolder: string
  tree: TreeState
  descriptions: Record<string, string> | undefined
  /** Absolute path of the active tab, highlighted in the tree. */
  activePath: string | null
  onOpenFile: (node: FinderNode) => void
  onPinFile: (node: FinderNode) => void
  onContext: (node: FinderNode, e: React.MouseEvent) => void
}) {
  return (
    // The rail is a deliberate twin of the thread list's: same `--sidebar-w`,
    // same `card-shadow my-2` panel with NO horizontal margin, same
    // `bg-surface-2`. Uniformity was the ask — a second rail vocabulary would
    // make the app feel like two apps.
    <aside className="card-shadow my-2 flex w-(--sidebar-w) shrink-0 flex-col overflow-hidden rounded-none border border-border bg-surface-2">
      <div
        className="flex h-full flex-col"
        style={{ width: "var(--sidebar-w)", minWidth: "var(--sidebar-w)" }}
      >
        <ScrollArea className="min-h-0 flex-1">
          {/* Vertical padding only. A tree row spans the FULL rail width — its
              highlight is a band, exactly as in VS Code — so horizontal padding
              here would inset every row and break that. The indent comes from
              the row's own guide spacers. */}
          <div className="py-1">
            <ExplorerChildren
              agentId={agentId}
              agentFolder={agentFolder}
              path={agentFolder}
              depth={0}
              tree={tree}
              descriptions={descriptions}
              activePath={activePath}
              onOpenFile={onOpenFile}
              onPinFile={onPinFile}
              onContext={onContext}
            />
          </div>
        </ScrollArea>
      </div>
    </aside>
  )
}

/**
 * One folder's children, and recursively the open folders among them.
 *
 * Mounted only while its folder is expanded — see {@link ExplorerTree}. The
 * recursion is HERE rather than in {@link ExplorerRow} so the row stays a
 * presentational leaf that can be rendered anywhere.
 */
function ExplorerChildren({
  agentId,
  agentFolder,
  path,
  depth,
  tree,
  descriptions,
  activePath,
  onOpenFile,
  onPinFile,
  onContext,
}: {
  agentId: string
  agentFolder: string
  /** Absolute path of the folder being listed. */
  path: string
  depth: number
  tree: TreeState
  descriptions: Record<string, string> | undefined
  activePath: string | null
  onOpenFile: (node: FinderNode) => void
  onPinFile: (node: FinderNode) => void
  onContext: (node: FinderNode, e: React.MouseEvent) => void
}) {
  const { data, loading } = useFs(agentId, relOf(agentFolder, path))
  const nodes = sortTreeNodes(data ?? [])

  if (loading && data === undefined) {
    return (
      <div
        className="flex h-[22px] items-center gap-1.5 text-[12px] text-muted-foreground/60"
        style={{ paddingLeft: `${String(depth * 9 + 22)}px` }}
      >
        <Loader2 className="size-3 animate-spin" />
        Loading…
      </div>
    )
  }

  if (nodes.length === 0) {
    return (
      <div
        className="flex h-[22px] items-center text-[12px] text-muted-foreground/45 italic"
        style={{ paddingLeft: `${String(depth * 9 + 22)}px` }}
      >
        empty
      </div>
    )
  }

  return (
    <>
      {nodes.map((node) => {
        const isFolder = node.kind === "folder"
        const expanded = tree.expanded.has(node.path)
        return (
          <div key={node.path}>
            <ExplorerRow
              node={node}
              depth={depth}
              expanded={expanded}
              active={node.path === activePath}
              description={descriptions?.[relOf(agentFolder, node.path)]}
              onActivate={() => {
                if (isFolder) tree.toggle(node.path)
                else onOpenFile(node)
              }}
              // A folder has nothing to pin, and passing a handler that toggles
              // twice would make a double click on a folder a no-op that looks
              // like a dropped input.
              onPin={isFolder ? undefined : () => onPinFile(node)}
              onContext={(e) => onContext(node, e)}
            />
            {isFolder && expanded && (
              <ExplorerChildren
                agentId={agentId}
                agentFolder={agentFolder}
                path={node.path}
                depth={depth + 1}
                tree={tree}
                descriptions={descriptions}
                activePath={activePath}
                onOpenFile={onOpenFile}
                onPinFile={onPinFile}
                onContext={onContext}
              />
            )}
          </div>
        )
      })}
    </>
  )
}
