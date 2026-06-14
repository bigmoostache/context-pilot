import { GitBranch, LayoutGrid, MessagesSquare } from "lucide-react"
import { PROJECT, status } from "@/lib/mock"
import { fmtCost } from "@/lib/panelMeta"
import { ThemeToggle } from "./ThemeToggle"
import type { ViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"

interface TopBarProps {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
}

/** Slim macOS-style title bar — app mark, view switcher, branch, cost, theme. */
export function TopBar({ view, onViewChange }: TopBarProps) {
  return (
    <header className="vibrancy flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
      {/* macOS traffic lights — purely decorative, sets the desktop-app tone */}
      <div className="flex items-center gap-2 pr-1">
        <span className="size-3 rounded-full bg-[#ff5f57]" />
        <span className="size-3 rounded-full bg-[#febc2e]" />
        <span className="size-3 rounded-full bg-[#28c840]" />
      </div>

      <div className="flex items-center gap-2">
        <span className="text-[13px] font-semibold tracking-tight text-foreground">
          Context Pilot
        </span>
      </div>

      {/* segmented view switcher */}
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
      </div>

      <span className="text-[12.5px] text-muted-foreground">{PROJECT.name}</span>

      <div className="ml-auto flex items-center gap-3">
        <div className="flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[12px] card-shadow">
          <GitBranch className="size-3.5 text-muted-foreground" />
          <span className="text-foreground/90">{status.branch}</span>
        </div>
        <span className="text-[12px] tabular-nums text-muted-foreground">
          {fmtCost(status.costUsd)}
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
