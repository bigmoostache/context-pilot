import { useState } from "react"
import { Activity, LayoutGrid, MessagesSquare, FolderTree, Home, Settings } from "lucide-react"
import { status, agents } from "@/lib/mock"
import { fmtCost } from "@/lib/panelMeta"
import { ThemeToggle } from "./ThemeToggle"
import { AgentSwitcher } from "./AgentSwitcher"
import { ConfigModal } from "./ConfigModal"
import { StatsPopup } from "./StatsPopup"
import type { ViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"

interface TopBarProps {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  activeAgentId: string
  onSwitchAgent: (id: string) => void
}

/** Slim macOS-style title bar — app mark (→ fleet), workspace switcher,
 *  per-agent view tabs (Threads · Cockpit · Finder), branch, cost, theme. */
export function TopBar({ view, onViewChange, activeAgentId, onSwitchAgent }: TopBarProps) {
  const activeAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]
  const inFleet = view === "fleet"
  const [configOpen, setConfigOpen] = useState(false)
  const [statsOpen, setStatsOpen] = useState(false)

  return (
    <header className="vibrancy flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
      {/* app mark → fleet dashboard (mission control) */}
      <button
        onClick={() => onViewChange("fleet")}
        className={cn(
          "flex items-center gap-1.5 rounded-md px-1.5 py-1 transition-colors",
          inFleet ? "text-foreground" : "text-foreground/90 hover:bg-muted/50",
        )}
        title="Fleet — mission control"
      >
        <Home className="size-4 text-[var(--signal)]" />
        <span className="text-[13px] font-semibold tracking-tight">Context Pilot</span>
      </button>

      {/* Workspace switcher — always present. Inside an agent it shows the
          active workspace; at fleet altitude (no agent focused) it falls back
          to a neutral "Select an agent" placeholder so the card never vanishes.
          Picking an agent here enters it (→ threads view). */}
      <span className="ml-1 text-muted-foreground/40">/</span>
      <AgentSwitcher
        activeId={inFleet ? undefined : activeAgentId}
        onSwitch={
          inFleet
            ? (id) => {
                onSwitchAgent(id)
                onViewChange("threads")
              }
            : onSwitchAgent
        }
        onFleet={() => onViewChange("fleet")}
      />

      {/* per-agent view switcher (hidden at fleet altitude) */}
      {!inFleet && (
        <div className="ml-2 flex items-center gap-0.5 rounded-lg border border-border bg-muted/60 p-0.5">
          <ViewTab
            active={view === "threads"}
            onClick={() => onViewChange("threads")}
            icon={MessagesSquare}
            label="Threads"
          />
          <ViewTab
            active={view === "cockpit"}
            onClick={() => onViewChange("cockpit")}
            icon={LayoutGrid}
            label="Cockpit"
          />
          <ViewTab
            active={view === "finder"}
            onClick={() => onViewChange("finder")}
            icon={FolderTree}
            label="Finder"
          />
        </div>
      )}

      <div className="ml-auto flex items-center gap-3">
        {/* cost is agent-scoped — only meaningful inside an agent */}
        {!inFleet && (
          <span className="text-[12px] tabular-nums text-muted-foreground">
            {fmtCost(activeAgent?.costUsd ?? status.costUsd)}
          </span>
        )}
        {/* session vitals are agent-scoped — irrelevant at fleet altitude */}
        {!inFleet && (
          <button
            onClick={() => setStatsOpen(true)}
            className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground"
            title="Session vitals"
            aria-label="Open session stats"
          >
            <Activity className="size-[17px]" />
          </button>
        )}
        <ThemeToggle />
        <span className="h-5 w-px bg-border/70" />
        <button
          onClick={() => setConfigOpen(true)}
          className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground"
          title="Settings"
          aria-label="Open settings"
        >
          <Settings className="size-[17px]" />
        </button>
      </div>

      <ConfigModal open={configOpen} onClose={() => setConfigOpen(false)} />
      <StatsPopup open={statsOpen} onClose={() => setStatsOpen(false)} />
    </header>
  )
}

function ViewTab({
  active,
  onClick,
  icon: Icon,
  label,
}: {
  active: boolean
  onClick: () => void
  icon: typeof MessagesSquare
  label: string
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12px] font-medium transition-all",
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
