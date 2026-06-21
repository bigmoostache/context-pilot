import { Check, ChevronsUpDown, FolderGit2, Plus } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { accentVar } from "@/lib/support/panelMeta"
import type { Agent, AgentStatus } from "@/lib/types"
import { cn } from "@/lib/utils"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs you", color: "var(--signal)" },
  idle: { label: "Idle", color: "var(--muted-foreground)" },
}

/**
 * Workspace switcher (1 agent = 1 folder). Sits at the left of the TopBar:
 * shows the active agent + its folder, opens a menu to switch between agents
 * or jump to the Agents launcher to browse the filesystem / create a new one.
 *
 * When no agent is focused (fleet altitude), pass `activeId` undefined: the
 * trigger keeps the same shape but shows a neutral **"Select an agent"**
 * placeholder, so the header card never disappears — it stays as a consistent,
 * always-available entry point to switch / create / manage agents.
 */
export function AgentSwitcher({
  agents,
  activeId,
  onSwitch,
  onFleet,
  onNewAgent,
}: {
  agents: Agent[]
  activeId?: string
  onSwitch: (id: string) => void
  /** Jump to the fleet dashboard — the only place agents are managed. */
  onFleet: () => void
  /** Jump to the fleet dashboard AND open the create-agent dialog. */
  onNewAgent: () => void
}) {
  const active = agents.find((a) => a.id === activeId)

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className={cn(
          "flex h-8 items-center gap-2 rounded-lg border border-border bg-card px-2.5 text-left transition-colors outline-none",
          "hover:border-[var(--signal)]/50 card-shadow",
        )}
      >
        {/* Single-line trigger: just the workspace name (the folder path was
            redundant noise — it lives in the menu rows + Finder). The fixed
            `h-8` locks the trigger to the exact height of the sibling
            Threads/Finder/Cockpit view-toggle pill group (also `h-8`). */}
        {active ? (
          <>
            <AgentDot accent={active.accent} status={active.status} compact />
            <span className="truncate text-[12.5px] font-semibold text-foreground/90">
              {active.name}
            </span>
          </>
        ) : (
          <>
            <PlaceholderDot />
            <span className="truncate text-[12.5px] font-semibold text-foreground/80">
              Select an agent
            </span>
          </>
        )}
        <ChevronsUpDown className="ml-1 size-3.5 shrink-0 text-muted-foreground/60" />
      </DropdownMenuTrigger>

      <DropdownMenuContent className="w-[280px]" align="start" sideOffset={6}>
        <DropdownMenuGroup>
          <DropdownMenuLabel className="text-[11px]">Workspaces · one agent per folder</DropdownMenuLabel>
          {agents.map((a) => (
            <DropdownMenuItem
              key={a.id}
              onClick={() => onSwitch(a.id)}
              className={cn(
                "flex items-center gap-2.5 py-1.5",
                // A refined, theme-correct highlight. base-ui marks the active
                // row with `data-highlighted` AND moves DOM focus onto it (the
                // reason the shadcn default `focus:bg-accent` fires) — we
                // override BOTH so the harsh stock accent is never seen. The
                // tint is a `color-mix` over transparent so it reads as a soft
                // brand wash that darkens correctly in dark mode and lightens in
                // light mode from the popover surface beneath, with the text
                // kept at full-contrast `foreground`.
                "focus:!bg-[color-mix(in_oklab,var(--signal)_11%,transparent)] focus:!text-foreground",
                "data-[highlighted]:!bg-[color-mix(in_oklab,var(--signal)_11%,transparent)] data-[highlighted]:!text-foreground",
              )}
            >
              <AgentDot accent={a.accent} status={a.status} />
              <div className="flex min-w-0 flex-1 leading-tight">
                <span className="truncate text-[12.5px] font-medium text-foreground/90">{a.name}</span>
              </div>
              <span
                className="shrink-0 text-[10px] font-medium"
                style={{ color: statusMeta[a.status].color }}
              >
                {statusMeta[a.status].label}
              </span>
              {a.id === activeId && <Check className="size-3.5 shrink-0 text-[var(--signal)]" />}
            </DropdownMenuItem>
          ))}
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuItem
            onClick={onNewAgent}
            className={cn(
              "gap-2 py-1.5 text-[12.5px]",
              "focus:!bg-muted data-[highlighted]:!bg-muted focus:!text-foreground data-[highlighted]:!text-foreground",
            )}
          >
            <Plus className="size-3.5 text-[var(--interactive)]" />
            New agent…
          </DropdownMenuItem>
          <DropdownMenuItem
            onClick={onFleet}
            className={cn(
              "gap-2 py-1.5 text-[12.5px]",
              "focus:!bg-muted data-[highlighted]:!bg-muted focus:!text-foreground data-[highlighted]:!text-foreground",
            )}
          >
            <FolderGit2 className="size-3.5 text-muted-foreground" />
            Manage agents (fleet)
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function AgentDot({
  accent,
  status,
  compact = false,
}: {
  accent: Agent["accent"]
  status: AgentStatus
  /** trigger variant — a smaller 20px glyph so the switcher matches the
   *  view-toggle pill height; the menu rows keep the default 28px dot. */
  compact?: boolean
}) {
  return (
    <span
      className={cn(
        "relative flex shrink-0 items-center justify-center",
        compact ? "size-5" : "size-7",
      )}
    >
      <span
        className={cn(
          "flex items-center justify-center rounded-md text-[11px] font-bold uppercase",
          compact ? "size-5" : "size-7",
        )}
        style={{
          background: `color-mix(in oklab, ${accentVar[accent]} 16%, transparent)`,
          color: accentVar[accent],
        }}
      >
        <FolderGit2 className={compact ? "size-3" : "size-3.5"} />
      </span>
      <span
        className={cn(
          "absolute -bottom-0.5 -right-0.5 size-2 rounded-full ring-2 ring-card",
          status === "working" && "animate-pulse",
        )}
        style={{ background: statusMeta[status].color }}
      />
    </span>
  )
}

/** Neutral trigger glyph shown when no agent is focused (fleet altitude). */
function PlaceholderDot() {
  return (
    <span className="flex size-5 shrink-0 items-center justify-center rounded-md border border-dashed border-border text-muted-foreground/55">
      <FolderGit2 className="size-3" />
    </span>
  )
}
