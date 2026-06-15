import { useState } from "react"
import { PanelLeft } from "lucide-react"
import { FleetSidebar, type FleetPage } from "./FleetSidebar"
import { FleetDashboard } from "./FleetDashboard"
import { PromptsPage } from "./PromptsPage"
import { UsagePage } from "./UsagePage"
import { ConfigPanel } from "@/components/shell/ConfigPanel"
import { Button } from "@/components/ui/button"

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
  const [collapsed, setCollapsed] = useState(false)

  return (
    <div className="relative flex min-h-0 flex-1">
      <FleetSidebar
        page={page}
        onSelect={setPage}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((v) => !v)}
      />
      {collapsed && (
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => setCollapsed(false)}
          title="Show sidebar"
          className="absolute left-2 top-2 z-10 border border-border bg-card text-muted-foreground card-shadow"
        >
          <PanelLeft className="size-4" />
        </Button>
      )}
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
