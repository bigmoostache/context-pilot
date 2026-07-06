import { useEffect, useState } from "react"
import { LayoutGrid, Library } from "lucide-react"
import { FleetDashboard } from "./FleetDashboard"
import { PromptsPage } from "./PromptsPage"
import { cn } from "@/lib/utils"
import type { Agent } from "@/lib/types"

/**
 * Fleet shell — the mission-control workspace shown when no agent is focused.
 *
 * There is **no sidebar** anymore: the dashboard collapsed to a single page
 * once its other sections found better homes —
 *   - **Agents** is the default landing (reached via the TopBar "Context Pilot"
 *     mark), and is now merged with **Prompts** into this one page, switched by
 *     a segmented toggle at the top.
 *   - **Usage** moved into the Settings dialog as its own category.
 *   - **Settings** is reachable only via the TopBar gear (a dialog).
 *
 * So this shell is just the merged **Agents ⇄ Prompts** surface with a top
 * toggle; both sub-pages are rendered full-bleed and untouched below it.
 */
type HomeTab = "agents" | "prompts"

/**
 * Shared content width for the whole fleet home — the toggle bar, the agent
 * grid and the prompt library all centre on this so their left/right edges
 * line up perfectly as you flip between tabs. Single source of truth.
 */
export const FLEET_MAX_W = "max-w-[960px]"

export function FleetShell({
  agents,
  onOpenAgent,
  openCreate,
  onCreateConsumed,
}: {
  agents: Agent[]
  onOpenAgent: (id: string) => void
  /** External request (from the workspace switcher's "New agent") to land on
   *  the Agents tab and pop the create dialog. */
  openCreate?: boolean
  onCreateConsumed?: () => void
}) {
  const [tab, setTab] = useState<HomeTab>("agents")

  // A "new agent" request must surface on the Agents tab — snap to it before
  // the dashboard opens its create dialog.
  useEffect(() => {
    if (openCreate) setTab("agents")
  }, [openCreate])

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-background">
      {/* top toggle — switch between the agent grid and the prompt library.
          The divider spans full width, but the segmented control is centred
          and padded to the SAME max-width / gutter as the page content below
          (FLEET_MAX_W · px-8), so its left edge lines up with the page heading. */}
      <div className="shrink-0 border-b border-border">
        <div className={cn("mx-auto flex h-[52px] w-full items-center px-8", FLEET_MAX_W)}>
          <div className="flex items-center gap-0.5 rounded-lg border border-border bg-muted/60 p-0.5">
            <ToggleTab
              active={tab === "agents"}
              onClick={() => setTab("agents")}
              icon={LayoutGrid}
              label="Agents"
            />
            <ToggleTab
              active={tab === "prompts"}
              onClick={() => setTab("prompts")}
              icon={Library}
              label="Prompts"
            />
          </div>
        </div>
      </div>

      {/* active sub-page (rendered untouched) */}
      {tab === "agents" ? (
        <FleetDashboard
          agents={agents}
          onOpenAgent={onOpenAgent}
          autoCreate={openCreate}
          onAutoCreateConsumed={onCreateConsumed}
        />
      ) : (
        <PromptsPage agentId={agents[0]?.id} />
      )}
    </div>
  )
}

function ToggleTab({
  active,
  onClick,
  icon: Icon,
  label,
}: {
  active: boolean
  onClick: () => void
  icon: typeof LayoutGrid
  label: string
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded-md px-3 py-1 text-[12.5px] font-medium transition-all",
        active
          ? "bg-card text-foreground card-shadow"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      <Icon className="size-3.5" />
      {label}
    </button>
  )
}
