import { Check, ChevronsUpDown, FolderGit2, Plus } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/mobile-components/ui/dropdown-menu"
import { accentVar } from "@/lib/support/panelMeta"
import { avatarUrl } from "@/lib/api"
import type { Agent, AgentStatus } from "@/lib/types"
import { cn } from "@/lib/utils"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs you", color: "var(--signal)" },
  idle: { label: "Idle", color: "var(--muted-foreground)" },
  disconnected: { label: "Disconnected", color: "var(--danger)" },
  waiting: { label: "Restarting", color: "var(--interactive)" },
}

/** Sort priority: working first, then needs-you, then idle. */
const statusOrder: Record<AgentStatus, number> = {
  working: 0,
  waiting: 0,
  "needs-you": 1,
  idle: 2,
  disconnected: 3,
}

/**
 * Workspace switcher (1 agent = 1 folder) — mobile twin of `components/shell/
 * widgets/AgentSwitcher`.
 *
 * Same dropdown surface as desktop, but sized for touch: the trigger grows from
 * the desktop `h-8` view-pill height to a `h-10` tap target, the menu opens
 * full-width (`w-[min(...)]`) instead of a fixed 280px popover so it reads as a
 * sheet on a phone, and every row is `py-2.5` (≥44px) with `active:` press
 * feedback replacing desktop `hover:`. Selection/sort/create logic is identical
 * — only the presentation forks. The dropdown primitive is the mobile
 * `@/mobile-components/ui/dropdown-menu` twin (action-sheet on mobile).
 */
export function AgentSwitcher({
  agents,
  activeId,
  onSwitch,
  onFleet,
  onNewAgent,
}: {
  agents: Agent[]
  activeId?: string | undefined
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
          "flex h-10 items-center gap-2 rounded-lg border border-border bg-card px-3 text-left outline-none",
          "card-shadow active:border-(--signal)/50",
        )}
      >
        {active ? (
          <>
            <AgentDot
              accent={active.accent}
              status={active.status}
              agentId={active.id}
              hasAvatar={active.hasAvatar}
              compact
            />
            <span className="truncate text-[14px] font-semibold text-foreground/90">
              {active.name}
            </span>
          </>
        ) : (
          <>
            <PlaceholderDot />
            <span className="truncate text-[14px] font-semibold text-foreground/80">
              Select an agent
            </span>
          </>
        )}
        <ChevronsUpDown className="ml-1 size-4 shrink-0 text-muted-foreground/60" />
      </DropdownMenuTrigger>

      <DropdownMenuContent
        className="w-[min(20rem,calc(100vw-1.5rem))]"
        align="start"
        sideOffset={6}
      >
        <DropdownMenuGroup>
          <DropdownMenuLabel className="text-[11px]">
            Workspaces · one agent per folder
          </DropdownMenuLabel>
          {agents
            .toSorted((a, b) => statusOrder[a.status] - statusOrder[b.status])
            .map((a) => (
              <DropdownMenuItem
                key={a.id}
                onClick={() => onSwitch(a.id)}
                className={cn(
                  "flex items-center gap-2.5 py-2.5",
                  // Override base-ui's stock highlight (data-highlighted + DOM
                  // focus) with a soft brand wash, same as desktop.
                  "focus:bg-[color-mix(in_oklab,var(--signal)_11%,transparent)]! focus:text-foreground!",
                  "data-highlighted:bg-[color-mix(in_oklab,var(--signal)_11%,transparent)]! data-highlighted:text-foreground!",
                )}
              >
                <AgentDot
                  accent={a.accent}
                  status={a.status}
                  agentId={a.id}
                  hasAvatar={a.hasAvatar}
                />
                <div className="flex min-w-0 flex-1 leading-tight">
                  <span className="truncate text-[13.5px] font-medium text-foreground/90 group-focus/dropdown-menu-item:text-foreground! group-data-highlighted/dropdown-menu-item:text-foreground!">
                    {a.name}
                  </span>
                </div>
                <span
                  className="shrink-0 text-[10.5px] font-medium"
                  style={{ color: statusMeta[a.status].color }}
                >
                  {statusMeta[a.status].label}
                </span>
                {a.id === activeId && <Check className="size-4 shrink-0 text-(--signal)" />}
              </DropdownMenuItem>
            ))}
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuItem
            onClick={onNewAgent}
            className={cn(
              "gap-2 py-2.5 text-[13.5px]",
              "focus:bg-muted! focus:text-foreground! data-highlighted:bg-muted! data-highlighted:text-foreground!",
            )}
          >
            <Plus className="size-4 text-(--interactive)" />
            New agent…
          </DropdownMenuItem>
          <DropdownMenuItem
            onClick={onFleet}
            className={cn(
              "gap-2 py-2.5 text-[13.5px]",
              "focus:bg-muted! focus:text-foreground! data-highlighted:bg-muted! data-highlighted:text-foreground!",
            )}
          >
            <FolderGit2 className="size-4 text-muted-foreground" />
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
  agentId,
  hasAvatar,
  compact = false,
}: {
  accent: Agent["accent"]
  status: AgentStatus
  agentId?: string | undefined
  hasAvatar?: boolean | undefined
  /** trigger variant — a smaller 20px glyph so the switcher matches the
   *  slim TopBar height; the menu rows keep the default 28px dot. */
  compact?: boolean
}) {
  return (
    <span
      className={cn(
        "relative flex shrink-0 items-center justify-center",
        compact ? "size-5" : "size-7",
      )}
    >
      {hasAvatar && agentId ? (
        <img
          src={avatarUrl(agentId)}
          alt=""
          className={cn("rounded-md object-cover", compact ? "size-5" : "size-7")}
        />
      ) : (
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
      )}
      <span
        className={cn(
          "absolute -right-0.5 -bottom-0.5 size-2 rounded-full ring-2 ring-card",
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
