import { useEffect } from "react"
import type { Agent, FinderNode } from "@/lib/types"
import { useTreeState } from "../explorer/treeState"
import { useEditorGroups } from "../editor/tabState"
import { kindOf } from "../support/kind"
import { FinderShell } from "./shell"

/**
 * Finder — a per-agent file manager confined to the agent's realm, reworked
 * (T624) into a VS Code shape: a nested explorer tree in the rail, a tabbed
 * file viewer in the content pane.
 *
 * The old macOS navigation personality (icon/list/column/gallery views, the
 * toolbar and path bar, Quick Look, rubber-band selection, the favourites
 * sidebar) is gone: in this idiom the TREE is the view and a TAB is the Quick
 * Look, so those surfaces had no counterpart to be ported into. What SURVIVED
 * untouched is the whole body layer under `preview/` plus the file-type icons —
 * a tab body simply is a `FinderPreview`, and how a file is reached is
 * orthogonal to how it is rendered.
 *
 * This component is the thin seam: {@link Finder} keys the body by agent id so
 * each agent mounts fresh, and {@link FinderBody} owns the two pieces of UI
 * state the explorer + tabs need ({@link useTreeState}, {@link useEditorGroups}
 * — the latter persists its open-tab / split layout per agent), threads the
 * "Show in Finder" reveal into the tree, and hands everything to
 * {@link FinderShell} for render.
 */
export function Finder(props: {
  agent: Agent
  /** Whether the explorer rail is expanded. Toggled from the header rail's
   *  Finder tab (T624); the tree slides out when false. */
  railOpen?: boolean
  /** Absolute realm path to reveal + open (T334 "Show in Finder"). */
  revealPath?: string | null | undefined
  onRevealConsumed?: (() => void) | undefined
  disconnected?: boolean
  onReconnect?: () => void
}) {
  // Key the whole body by agent id. Switching agents (or reloading the page)
  // then remounts it from scratch, so `useEditorGroups`/`useTreeState` hydrate
  // that agent's own saved layout with no cross-agent state bleed and no
  // effect-ordering race on the persisted groups (T630 — tabs were lost on view
  // switch + reload because this state lived only in unmounting `useState`).
  return <FinderBody key={props.agent.id} {...props} />
}

function FinderBody({
  agent,
  railOpen = true,
  revealPath,
  onRevealConsumed,
  disconnected,
  onReconnect,
}: {
  agent: Agent
  railOpen?: boolean
  revealPath?: string | null | undefined
  onRevealConsumed?: (() => void) | undefined
  disconnected?: boolean
  onReconnect?: () => void
}) {
  const tree = useTreeState()
  const groups = useEditorGroups(agent.id)

  // "Show in Finder": expand every ancestor of the target so its row renders,
  // then open it pinned (the user asked to SEE this file, not to glance at it).
  // `tree` and `groups` are memoised bundles and `onRevealConsumed` is guarded
  // by the `revealPath` check, so listing them all keeps exhaustive-deps happy
  // while the effect still fires only when a fresh path arrives — the parent
  // nulls `revealPath` on consume, so the reveal runs exactly once.
  useEffect(() => {
    if (!revealPath) return
    tree.reveal(revealPath, agent.folder)
    const name = revealPath.split("/").at(-1) ?? revealPath
    const node: FinderNode = { name, path: revealPath, kind: kindOf(name), modified: "" }
    groups.openPinned(node)
    onRevealConsumed?.()
  }, [revealPath, agent.folder, tree, groups, onRevealConsumed])

  return (
    <FinderShell
      agent={agent}
      tree={tree}
      groups={groups}
      railOpen={railOpen}
      disconnected={disconnected}
      onReconnect={onReconnect}
    />
  )
}
