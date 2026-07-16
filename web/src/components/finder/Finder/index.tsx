import { useRef } from "react"
import type { Agent } from "@/lib/types"
import { req } from "../internal/helpers"
import { useFinderViewState, useFinderListing } from "./state"
import { useFinderController } from "./controller"
import { FinderShell } from "./shell"

export type { PinnedFolder } from "../internal/helpers"

/**
 * Finder — a per-agent file manager confined to the agent's realm. Tabs +
 * history navigation, four view modes (grid / list / Miller columns / gallery),
 * full keyboard control (arrows, type-ahead, Space QuickLook, Enter rename),
 * range + additive selection, a right-click context menu, an icon-size slider,
 * a QuickLook Sheet drawer, a toggleable path bar, internal drag-and-drop moves,
 * and a drag-and-drop upload affordance. Live data via the orchestration plane.
 *
 * Structure (P8): the view state lives in {@link useFinderViewState}, the live
 * listing + derivations in {@link useFinderListing}, every behaviour hook +
 * derived handler surface in {@link useFinderController}, and the whole render
 * in {@link FinderShell}. So this component is a thin seam that resolves the
 * active tab and wires those four together — each unit within the P8
 * line/statement/complexity budgets.
 */
export function Finder({
  agent,
  revealPath,
  onRevealConsumed,
  disconnected,
  onReconnect,
}: {
  agent: Agent
  revealPath?: string | null | undefined
  onRevealConsumed?: (() => void) | undefined
  disconnected?: boolean
  onReconnect?: () => void
}) {
  const surfaceRef = useRef<HTMLDivElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const vs = useFinderViewState(agent)

  // `tabs` is seeded with one tab and closeTab never empties it, so index 0 is
  // always present — assert it so `active` is `Tab`, not `Tab | undefined`.
  const active = vs.tabs.find((t) => t.id === vs.activeId) ?? req(vs.tabs, 0)
  const cwd = active.cwd

  const listing = useFinderListing({
    agentId: agent.id,
    agentFolder: agent.folder,
    agentName: agent.name,
    cwd,
    query: vs.query,
    sortKey: vs.sortKey,
    asc: vs.asc,
    pendingFolderName: vs.pendingFolderName,
    preview: vs.preview,
    focusPath: vs.focusPath,
  })

  const ctrl = useFinderController({
    agent,
    vs,
    listing,
    active,
    cwd,
    surfaceRef,
    fileInputRef,
    revealPath,
    onRevealConsumed,
  })

  return (
    <FinderShell
      agent={agent}
      vs={vs}
      listing={listing}
      active={active}
      cwd={cwd}
      ctrl={ctrl}
      surfaceRef={surfaceRef}
      fileInputRef={fileInputRef}
      disconnected={disconnected}
      onReconnect={onReconnect}
    />
  )
}
