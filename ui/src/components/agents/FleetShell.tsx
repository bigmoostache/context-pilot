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
  const [collapsed, setCollapsed] = useState(false)

  return (
    <div className="relative flex min-h-0 flex-1">
      <FleetSidebar page={page} onSelect={setPage} collapsed={collapsed} />

      {/* Collapse rail — click the sidebar's right edge to collapse/expand it
          (the shadcn Sidebar interaction). A generous hit zone hugs the border;
          on hover a soft band lights up and a pill-grip handle appears so the
          affordance reads clearly. Tracks the sidebar width, stays reachable at
          x=0 when collapsed, and flips a chevron to hint the action. */}
      <button
        onClick={() => setCollapsed((v) => !v)}
        title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        className="group absolute inset-y-0 z-20 w-5 -translate-x-1/2 cursor-pointer transition-[left] duration-200 ease-in-out"
        style={{ left: collapsed ? 6 : "var(--sidebar-w)" }}
      >
        {/* hover band — a subtle highlight across the seam */}
        <span className="absolute inset-y-0 left-1/2 w-[3px] -translate-x-1/2 rounded-full bg-border transition-all duration-150 group-hover:w-[5px] group-hover:bg-[var(--interactive)]/45 group-active:bg-[var(--interactive)]/70" />
        {/* pill-grip handle — the obvious drag/click affordance */}
        <span className="absolute top-1/2 left-1/2 flex h-11 w-[18px] -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border border-border bg-card opacity-0 shadow-sm transition-all duration-150 group-hover:opacity-100 group-active:scale-95">
          <span className="flex flex-col items-center gap-[3px]">
            <span className="size-[3px] rounded-full bg-muted-foreground/60" />
            <span className="size-[3px] rounded-full bg-muted-foreground/60" />
            <span className="size-[3px] rounded-full bg-muted-foreground/60" />
          </span>
        </span>
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
