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
          "flex items-center gap-2 rounded-lg border border-border bg-card px-2.5 py-1.5 text-left transition-colors outline-none",
          "hover:border-[var(--signal)]/50 card-shadow",
        )}
      >
        {active ? (
          <>
            <AgentDot accent={active.accent} status={active.status} />
            <div className="flex min-w-0 flex-col leading-tight">
              <span className="truncate text-[12.5px] font-semibold text-foreground/90">
                {active.name}
              </span>
              <span className="truncate font-mono text-[10px] text-muted-foreground/70">
                {active.folder}
              </span>
            </div>
          </>
        ) : (
          <>
            <PlaceholderDot />
            <div className="flex min-w-0 flex-col leading-tight">
              <span className="truncate text-[12.5px] font-semibold text-foreground/80">
                Select an agent
              </span>
              <span className="truncate text-[10px] text-muted-foreground/65">
                Choose a workspace
              </span>
            </div>
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
              className="flex items-center gap-2.5 py-1.5"
            >
              <AgentDot accent={a.accent} status={a.status} />
              <div className="flex min-w-0 flex-1 flex-col leading-tight">
                <span className="truncate text-[12.5px] font-medium text-foreground/90">{a.name}</span>
                <span className="truncate font-mono text-[10px] text-muted-foreground/65">{a.folder}</span>
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
          <DropdownMenuItem onClick={onNewAgent} className="gap-2 py-1.5 text-[12.5px]">
            <Plus className="size-3.5 text-[var(--interactive)]" />
            New agent…
          </DropdownMenuItem>
          <DropdownMenuItem onClick={onFleet} className="gap-2 py-1.5 text-[12.5px]">
            <FolderGit2 className="size-3.5 text-muted-foreground" />
            Manage agents (fleet)
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function AgentDot({ accent, status }: { accent: Agent["accent"]; status: AgentStatus }) {
  return (
    <span className="relative flex size-7 shrink-0 items-center justify-center">
      <span
        className="flex size-7 items-center justify-center rounded-md text-[11px] font-bold uppercase"
        style={{
          background: `color-mix(in oklab, ${accentVar[accent]} 16%, transparent)`,
          color: accentVar[accent],
        }}
      >
        <FolderGit2 className="size-3.5" />
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
    <span className="flex size-7 shrink-0 items-center justify-center rounded-md border border-dashed border-border text-muted-foreground/55">
      <FolderGit2 className="size-3.5" />
    </span>
  )
}
