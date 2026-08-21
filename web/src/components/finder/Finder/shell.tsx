import type { Agent, FinderNode } from "@/lib/types"
import { clickable } from "@/lib/support/a11y"
import { useFsDescriptions } from "@/lib/live"
import { ExplorerTree } from "../explorer/ExplorerTree"
import { TabHost } from "../editor/TabHost"
import type { TreeState } from "../explorer/treeState"
import type { TabsState } from "../editor/tabState"

/**
 * The Finder's two-pane render: the explorer tree on the left, the tabbed file
 * viewer on the right, with the disconnect overlay layered over both.
 *
 * Extracted from {@link Finder} so the seam component stays a thin wiring layer.
 * Deliberately spare — the macOS chrome (toolbar, path bar, status bar, marquee
 * band, context menu) that the old shell orchestrated is gone with the views it
 * served.
 */
export function FinderShell({
  agent,
  tree,
  tabs,
  railOpen = true,
  disconnected,
  onReconnect,
}: {
  agent: Agent
  tree: TreeState
  tabs: TabsState
  /** Whether the explorer rail is expanded. When false the tree slides off the
   *  left edge (the {@link FinderShell} root clips it) and the tab host reclaims
   *  the width — the Finder twin of the Threads/Settings rail collapse. */
  railOpen?: boolean
  disconnected?: boolean | undefined
  onReconnect?: (() => void) | undefined
}) {
  // The agent's tree descriptions (realm-relative path → text), for the ⓘ badge
  // on described rows. One fetch per agent, shared across the whole tree.
  const { data: descriptions } = useFsDescriptions(agent.id)

  return (
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
          activePath={tabs.activePath}
          onOpenFile={(node: FinderNode) => tabs.openPreview(node)}
          onPinFile={(node: FinderNode) => tabs.openPinned(node)}
          // A context menu is a follow-up (T624 P-later); the tree is fully
          // usable by click alone in the meantime, so the handler is a no-op
          // rather than a half-built menu.
          onContext={() => {
            /* no context menu yet */
          }}
        />
      </div>

      <TabHost tabs={tabs} agentId={agent.id} />
    </div>
  )
}
