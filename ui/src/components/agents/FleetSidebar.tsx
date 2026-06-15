import { LayoutGrid, Coins, Library, Settings2 } from "lucide-react"
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

/** Fixed inner width so content doesn't reflow while the rail collapses. */
const SIDEBAR_W = 212

/**
 * Mission-control sidebar — the navigation rail shown when no agent is focused.
 * Selects between the dashboard's four pages (Agents / Prompts / Usage /
 * Settings). The Context-Pilot identity is intentionally *not* repeated here —
 * it already lives in the TopBar.
 *
 * Collapsing is driven entirely by the **draggable-looking border rail** that
 * {@link FleetShell} renders on this aside's right edge (the shadcn `Sidebar`
 * pattern) — there are no in-rail collapse buttons. This component only reacts
 * to the `collapsed` flag by animating its width to zero.
 */
export function FleetSidebar({
  page,
  onSelect,
  collapsed,
}: {
  page: FleetPage
  onSelect: (p: FleetPage) => void
  collapsed: boolean
}) {
  return (
    <aside
      className={cn(
        "flex shrink-0 flex-col overflow-hidden bg-surface transition-[width] duration-200 ease-in-out",
        collapsed ? "w-0 border-r-0" : "w-[212px] border-r border-border",
      )}
    >
      <div
        className="flex h-full flex-col"
        style={{ width: SIDEBAR_W, minWidth: SIDEBAR_W }}
      >
        {/* nav */}
        <nav className="flex min-h-0 flex-1 flex-col gap-0.5 px-2.5 pb-1 pt-4">
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
      </div>
    </aside>
  )
}
