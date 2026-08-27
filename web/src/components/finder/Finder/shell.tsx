import type { Agent, FinderNode } from "@/lib/types"
import {
  DndContext,
  PointerSensor,
  pointerWithin,
  closestCenter,
  useSensor,
  useSensors,
  type CollisionDetection,
} from "@dnd-kit/core"
import { clickable } from "@/lib/support/a11y"
import { useFsDescriptions } from "@/lib/live"
import { ExplorerTree } from "../explorer/ExplorerTree"
import { TabHost } from "../editor/TabHost"
import type { TreeState } from "../explorer/treeState"
import type { GroupsState } from "../editor/tabState"

/**
 * Pointer-first collision: drop WHERE THE POINTER IS, falling back to
 * closest-center only when the pointer is over no droppable.
 *
 * Plain `closestCenter` is unambiguous with ONE editor group (the single old
 * P2 case, which worked) but breaks the moment there are two or more: a small
 * explorer row dragged into split B can have its rect-center nearest to group
 * A's (or a tab's) center, so the file opened in the wrong pane — or, if the
 * center landed between panes, in none, and the drop silently no-op'd. That is
 * exactly "can't drag into the split view". `pointerWithin` keys off the cursor
 * position instead, so the file lands in the pane under the pointer; the
 * `closestCenter` tail keeps tab-reorder forgiving when the cursor slips just
 * past a strip.
 */
const pointerFirst: CollisionDetection = (args) => {
  const hits = pointerWithin(args)
  return hits.length > 0 ? hits : closestCenter(args)
}

/**
 * The Finder's two-pane render: the explorer tree on the left, the split
 * editor (one or more groups side by side) on the right, with the disconnect
 * overlay layered over both.
 *
 * Extracted from {@link Finder} so the seam component stays a thin wiring layer.
 * Deliberately spare — the macOS chrome (toolbar, path bar, status bar, marquee
 * band, context menu) that the old shell orchestrated is gone with the views it
 * served.
 */
export function FinderShell({
  agent,
  tree,
  groups,
  railOpen = true,
  disconnected,
  onReconnect,
}: {
  agent: Agent
  tree: TreeState
  groups: GroupsState
  /** Whether the explorer rail is expanded. When false the tree slides off the
   *  left edge (the {@link FinderShell} root clips it) and the editor reclaims
   *  the width — the Finder twin of the Threads/Settings rail collapse. */
  railOpen?: boolean
  disconnected?: boolean | undefined
  onReconnect?: (() => void) | undefined
}) {
  // The agent's tree descriptions (realm-relative path → text), for the ⓘ badge
  // on described rows. One fetch per agent, shared across the whole tree.
  const { data: descriptions } = useFsDescriptions(agent.id)

  // ONE DndContext spanning the explorer AND every editor group (T630 P1–P3).
  // It hosts three drag systems: each group's tab SortableContext (reorder),
  // cross-group tab moves, and the explorer's file-row draggables. The whole
  // dispatch lives in the groups model — `applyDragEnd` sorts explorer-open vs
  // in-group reorder vs cross-group move — so the shell stays a thin delegate.
  // A 4px activation distance keeps every click affordance (open, toggle,
  // close) intact: a plain click never starts a drag.
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }))

  return (
    <DndContext sensors={sensors} collisionDetection={pointerFirst} onDragEnd={groups.applyDragEnd}>
      <div
        className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"
        style={
          disconnected
            ? { filter: "blur(3px) grayscale(0.5)", transition: "filter 300ms" }
            : { transition: "filter 300ms" }
        }
      >
        {disconnected && (
          <div
            {...clickable(() => onReconnect?.())}
            aria-label="Reconnect agent"
            className="absolute inset-0 z-40 cursor-pointer bg-background/30"
          />
        )}

        {/* Slide wrapper (T624): the tree stays MOUNTED and slides off the left
            edge via a transitioned negative margin when the rail is collapsed —
            the same mechanism the Threads/Settings rails use, so tree expansion
            state and the lazily-fetched folder listings survive a hide/show. A
            `flex` wrapper (not a plain block) so the aside inside keeps its
            flex-stretch height. `motion-reduce` snaps instead of sliding. */}
        <div
          className="flex transition-[margin] duration-200 ease-out motion-reduce:transition-none"
          style={{ marginLeft: railOpen ? 0 : "calc(-1 * var(--sidebar-w))" }}
          aria-hidden={!railOpen}
        >
          <ExplorerTree
            agentId={agent.id}
            agentFolder={agent.folder}
            tree={tree}
            descriptions={descriptions}
            activePath={groups.activePath}
            onOpenFile={(node: FinderNode) => groups.openPreview(node)}
            onPinFile={(node: FinderNode) => groups.openPinned(node)}
            // A context menu is a follow-up (T624 P-later); the tree is fully
            // usable by click alone in the meantime, so the handler is a no-op
            // rather than a half-built menu.
            onContext={() => {
              /* no context menu yet */
            }}
          />
        </div>

        <TabHost groups={groups} agentId={agent.id} />
      </div>
    </DndContext>
  )
}
