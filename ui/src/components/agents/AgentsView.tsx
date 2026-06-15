import { useState } from "react"
import { ArrowLeft } from "lucide-react"
import { FileBrowser } from "./FileBrowser"
import { AgentDetail } from "./AgentDetail"
import { FleetDashboard } from "./FleetDashboard"
import { fileTree, agents } from "@/lib/mock"
import type { FsNode } from "@/lib/types"

/**
 * Agents launcher — "mission control" for workspaces (1 agent = 1 folder).
 *
 * Two surfaces:
 *  • **fleet** (landing) — a welcome dashboard of every agent with thread stats.
 *  • **browse** — the filesystem tree + detail pane, used to open an agent or
 *    initialize one in a plain folder.
 */
export function AgentsView({
  onOpenAgent,
}: {
  onOpenAgent: (id: string) => void
}) {
  const [mode, setMode] = useState<"fleet" | "browse">("fleet")
  const [selected, setSelected] = useState<FsNode | null>(null)

  // Enter browse mode, optionally pre-selecting a folder (e.g. an agent's realm).
  const browseAt = (path?: string) => {
    setSelected(path ? findByPath(fileTree, path) : null)
    setMode("browse")
  }

  if (mode === "fleet") {
    return (
      <div className="flex min-h-0 flex-1">
        <FleetDashboard
          onOpenAgent={onOpenAgent}
          onManageAgent={(id) => browseAt(agents.find((a) => a.id === id)?.folder)}
          onNewAgent={() => browseAt()}
        />
      </div>
    )
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* back bar — return to the fleet welcome dashboard */}
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border bg-surface px-3">
        <button
          onClick={() => setMode("fleet")}
          className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] font-medium text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
        >
          <ArrowLeft className="size-3.5" />
          All agents
        </button>
        <span className="text-[12px] text-muted-foreground/50">Browse filesystem</span>
      </div>
      <div className="flex min-h-0 flex-1">
        <FileBrowser root={fileTree} selectedPath={selected?.path ?? ""} onSelect={setSelected} />
        <AgentDetail node={selected} onOpenAgent={onOpenAgent} />
      </div>
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
