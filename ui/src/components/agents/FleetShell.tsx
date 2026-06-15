import { useState } from "react"
import { FleetSidebar, type FleetPage } from "./FleetSidebar"
import { FleetDashboard } from "./FleetDashboard"
import { PromptsPage } from "./PromptsPage"
import { UsagePage } from "./UsagePage"
import { ConfigPanel } from "@/components/shell/ConfigPanel"

/** Sidebar width — kept in sync with FleetSidebar's SIDEBAR_W so the rail lands on the border. */
const SIDEBAR_W = 212

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
      <FleetSidebar page={page} onSelect={setPage} collapsed={collapsed} />

      {/* Border rail — click the sidebar's right edge to collapse/expand it
          (the shadcn Sidebar interaction). Tracks the sidebar width so it
          always hugs the border, and stays reachable at x=0 when collapsed. */}
      <button
        onClick={() => setCollapsed((v) => !v)}
        title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        className="group absolute inset-y-0 z-20 w-3 -translate-x-1/2 cursor-pointer transition-[left] duration-200 ease-in-out"
        style={{ left: collapsed ? 0 : SIDEBAR_W }}
      >
        <span className="absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-border transition-colors group-hover:bg-[var(--interactive)]/70 group-active:bg-[var(--interactive)]" />
      </button>

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
