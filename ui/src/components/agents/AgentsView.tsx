import { useState } from "react"
import { FileBrowser } from "./FileBrowser"
import { AgentDetail } from "./AgentDetail"
import { fileTree, agents } from "@/lib/mock"
import type { FsNode } from "@/lib/types"

/**
 * Agents launcher — the "mission control" for workspaces. Each agent is one
 * folder. Browse the filesystem on the left; the right pane lets you open an
 * existing agent or initialize one in a plain folder. Selecting "Open" switches
 * the active agent and drops you into its threads view.
 */
export function AgentsView({
  activeAgentId,
  onOpenAgent,
}: {
  activeAgentId: string
  onOpenAgent: (id: string) => void
}) {
  // Default selection: the active agent's folder.
  const activeFolder = agents.find((a) => a.id === activeAgentId)?.folder ?? ""
  const [selected, setSelected] = useState<FsNode | null>(() => findByPath(fileTree, activeFolder))

  return (
    <div className="flex min-h-0 flex-1">
      <FileBrowser
        root={fileTree}
        selectedPath={selected?.path ?? ""}
        onSelect={setSelected}
      />
      <AgentDetail node={selected} onOpenAgent={onOpenAgent} />
    </div>
  )
}

/** Depth-first search for a node by its path. */
function findByPath(node: FsNode, path: string): FsNode | null {
  if (node.path === path) return node
  for (const child of node.children ?? []) {
    const hit = findByPath(child, path)
    if (hit) return hit
  }
  return null
}
