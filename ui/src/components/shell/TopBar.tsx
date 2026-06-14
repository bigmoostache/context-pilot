import { GitBranch, LayoutGrid, MessagesSquare, Plane } from "lucide-react"
import { PROJECT, status } from "@/lib/mock"
import { fmtCost } from "@/lib/panelMeta"
import type { ViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"

interface TopBarProps {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
}

/** Thin cockpit header strip — app mark, view switcher, branch, global cost. */
export function TopBar({ view, onViewChange }: TopBarProps) {
  return (
    <header className="flex h-9 shrink-0 items-center gap-3 border-b border-border bg-[oklch(0.18_0.007_75)] px-3 etch">
      <div className="flex items-center gap-2">
        <Plane className="size-3.5 -rotate-45 text-[var(--signal)]" strokeWidth={2.25} />
        <span className="glow-signal text-[12px] font-semibold tracking-[0.22em] text-[var(--signal)]">
          CONTEXT·PILOT
        </span>
      </div>

      <span className="h-3.5 w-px bg-border" />

      {/* view switcher */}
      <div className="flex items-center gap-0.5 rounded-[4px] border border-border bg-[oklch(0.165_0.006_75)] p-0.5">
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

      <nav className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <span className="text-[var(--grid)]">/</span>
        <span className="text-foreground/80">{PROJECT.name}</span>
      </nav>

      <div className="ml-auto flex items-center gap-3">
        <div className="flex items-center gap-1.5 rounded-[3px] border border-border bg-card px-2 py-0.5 text-[11px]">
          <GitBranch className="size-3 text-[var(--interactive)]" />
          <span className="text-foreground/90">{status.branch}</span>
        </div>
        <div className="flex items-center gap-1.5 text-[11px]">
          <span className="label">cost</span>
          <span className="font-semibold text-[var(--warn)]">{fmtCost(status.costUsd)}</span>
        </div>
        <span className="size-2 rounded-full bg-[var(--ok)] shadow-[0_0_6px_var(--ok)]" />
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
        "flex items-center gap-1.5 rounded-[3px] px-2 py-0.5 text-[11px] font-medium transition-colors",
        active
          ? "bg-[var(--signal)]/15 text-[var(--signal)] glow-signal"
          : "text-muted-foreground hover:text-foreground/80",
      )}
    >
      <Icon className="size-3" />
      {label}
    </button>
  )
}
