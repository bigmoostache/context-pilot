import { useRef } from "react"
import type { Agent } from "@/lib/types"
import { req } from "../internal/helpers"
import { useFinderViewState, useFinderListing } from "./state"
import { useFinderController } from "./controller"
import { FinderShell } from "./shell"

export type { PinnedFolder } from "../internal/helpers"

/**
 * Finder — mobile twin of `components/finder/Finder`. Identical seam: resolve
 * the active tab, wire the shared view-state / listing / controller hooks (all
 * kept as @generated stubs re-exporting desktop, so the behaviour is byte-shared)
 * into {@link FinderShell}. The whole mobile divergence lives downstream in the
 * shell / body / views (single-pane column stack, touch tap contract, drawer
 * sidebar); this file is an ancestor promoted to route those mobile children.
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
