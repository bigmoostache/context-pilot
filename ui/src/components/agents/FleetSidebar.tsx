import { Home, LayoutGrid, Coins, Library, Settings2 } from "lucide-react"
import { cn } from "@/lib/utils"

/** The four sections of the fleet (no-agent-focused) dashboard. */
export type FleetPage = "agents" | "prompts" | "usage" | "settings"

const NAV: {
  id: FleetPage
  label: string
  icon: typeof LayoutGrid
  hint: string
}[] = [
  { id: "agents", label: "Agents", icon: LayoutGrid, hint: "Your fleet" },
  { id: "prompts", label: "Prompts", icon: Library, hint: "Skills · agents · commands" },
  { id: "usage", label: "Usage", icon: Coins, hint: "Cost & tokens" },
  { id: "settings", label: "Settings", icon: Settings2, hint: "Providers & config" },
]

/**
 * Mission-control sidebar — the navigation rail shown when no agent is focused.
 * Selects between the dashboard's four pages (Agents / Prompts / Usage /
 * Settings). Mirrors the macOS-sidebar register used elsewhere in the maquette.
 */
export function FleetSidebar({
  page,
  onSelect,
}: {
  page: FleetPage
  onSelect: (p: FleetPage) => void
}) {
  return (
    <aside className="flex w-[212px] shrink-0 flex-col border-r border-border bg-surface">
      {/* identity */}
      <div className="flex items-center gap-2.5 px-4 pb-4 pt-5">
        <span className="flex size-9 items-center justify-center rounded-xl bg-[var(--signal)]/14 text-[var(--signal)] ring-1 ring-inset ring-[var(--signal)]/25">
          <Home className="size-[18px]" />
        </span>
        <div className="flex min-w-0 flex-col">
          <span className="truncate text-[13.5px] font-semibold tracking-tight text-foreground">Context Pilot</span>
          <span className="truncate text-[11px] text-muted-foreground">Mission control</span>
        </div>
      </div>

      <div className="mx-3 h-px bg-border/60" />

      {/* nav */}
      <nav className="flex min-h-0 flex-1 flex-col gap-0.5 px-2.5 py-3">
        {NAV.map((n) => {
          const on = n.id === page
          return (
            <button
              key={n.id}
              onClick={() => onSelect(n.id)}
              className={cn(
                "group flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left transition-colors",
                on ? "bg-card card-shadow" : "hover:bg-muted/60",
              )}
            >
              <span
                className={cn(
                  "flex size-7 shrink-0 items-center justify-center rounded-md transition-colors",
                  on ? "bg-[var(--signal)]/15 text-[var(--signal)]" : "text-muted-foreground/70 group-hover:text-foreground",
                )}
              >
                <n.icon className="size-[16px]" />
              </span>
              <span className="flex min-w-0 flex-1 flex-col leading-tight">
                <span className={cn("text-[12.5px]", on ? "font-medium text-foreground" : "text-foreground/75")}>
                  {n.label}
                </span>
                <span className="truncate text-[10px] text-muted-foreground/60">{n.hint}</span>
              </span>
            </button>
          )
        })}
      </nav>

      <div className="px-4 py-3 text-[10px] leading-relaxed text-muted-foreground/50">
        Design-only maquette.
      </div>
    </aside>
  )
}
