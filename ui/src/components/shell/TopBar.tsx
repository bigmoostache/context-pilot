import { GitBranch, LayoutGrid, MessagesSquare, FolderTree, Home } from "lucide-react"
import { status, agents } from "@/lib/mock"
import { fmtCost } from "@/lib/panelMeta"
import { ThemeToggle } from "./ThemeToggle"
import { AgentSwitcher } from "./AgentSwitcher"
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

      {/* workspace switcher — pick which agent (folder) you're working in */}
      <span className="ml-1 text-muted-foreground/40">/</span>
      <AgentSwitcher
        activeId={activeAgentId}
        onSwitch={onSwitchAgent}
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
        <div className="flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[12px] card-shadow">
          <GitBranch className="size-3.5 text-muted-foreground" />
          <span className="text-foreground/90">{activeAgent?.branch ?? status.branch}</span>
        </div>
        <span className="text-[12px] tabular-nums text-muted-foreground">
          {fmtCost(activeAgent?.costUsd ?? status.costUsd)}
        </span>
        <ThemeToggle />
      </div>
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
