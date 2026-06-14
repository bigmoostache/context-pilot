import { GitBranch, Plane } from "lucide-react"
import { PROJECT, status } from "@/lib/mock"
import { fmtCost } from "@/lib/panelMeta"

/** Thin cockpit header strip — app mark, breadcrumb, branch, global cost. */
export function TopBar() {
  return (
    <header className="flex h-9 shrink-0 items-center gap-3 border-b border-border bg-[oklch(0.18_0.007_75)] px-3 etch">
      <div className="flex items-center gap-2">
        <Plane className="size-3.5 -rotate-45 text-[var(--signal)]" strokeWidth={2.25} />
        <span className="glow-signal text-[12px] font-semibold tracking-[0.22em] text-[var(--signal)]">
          CONTEXT·PILOT
        </span>
      </div>

      <span className="h-3.5 w-px bg-border" />

      <nav className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <span>{PROJECT.path}</span>
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
