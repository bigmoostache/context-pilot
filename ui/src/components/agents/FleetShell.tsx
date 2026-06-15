import { useState } from "react"
import { FleetSidebar, type FleetPage } from "./FleetSidebar"
import { FleetDashboard } from "./FleetDashboard"
import { PromptsPage } from "./PromptsPage"
import { UsagePage } from "./UsagePage"
import { ConfigPanel } from "@/components/shell/ConfigPanel"

/**
 * Fleet shell — the mission-control workspace shown when no agent is focused.
 * A persistent {@link FleetSidebar} on the left navigates the four sections;
 * the right pane swaps the active page:
 *
 *  - **agents**   → {@link FleetDashboard} (the agent grid + create/manage)
 *  - **prompts**  → {@link PromptsPage} (global skills / agents / commands)
 *  - **usage**    → {@link UsagePage} (cost & token analytics)
 *  - **settings** → {@link ConfigPanel} inline (same body the TopBar gear opens
 *                    in a dialog — here it lives directly in the page)
 */
export function FleetShell({ onOpenAgent }: { onOpenAgent: (id: string) => void }) {
  const [page, setPage] = useState<FleetPage>("agents")

  return (
    <div className="flex min-h-0 flex-1">
      <FleetSidebar page={page} onSelect={setPage} />
      {page === "agents" ? (
        <FleetDashboard onOpenAgent={onOpenAgent} />
      ) : page === "prompts" ? (
        <PromptsPage />
      ) : page === "usage" ? (
        <UsagePage />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col bg-background">
          <ConfigPanel variant="inline" />
        </div>
      )}
    </div>
  )
}
