import { ChevronDown, FolderGit2, LayoutGrid } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
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
 * Workspace switcher (1 agent = 1 folder). Sits at the left of the TopBar:
 * shows the active agent + its folder, opens a menu to switch between agents.
 * Fleet management (create / manage) lives solely in the fleet view, reached
 * via the TopBar's home button — the menu is now a pure switch list (T676).
 *
 * When no agent is focused (fleet altitude), pass `activeId` undefined: the
 * trigger keeps the same shape but shows a neutral **"Select an agent"**
 * placeholder, so the header entry point never disappears.
 */
export function AgentSwitcher({
  agents,
  activeId,
  onSwitch,
  onManageAgents,
  rail = false,
}: {
  agents: Agent[]
  activeId?: string | undefined
  onSwitch: (id: string) => void
  /** open the fleet dashboard — the sole place agents are created/managed
   *  (T685: this replaces the removed TopBar home button). */
  onManageAgents?: (() => void) | undefined
  /** Vertical-rail variant: the trigger collapses to the agent glyph alone.
   *  The rail is 56px wide, so the workspace NAME and the chevron have nowhere
   *  to go — and truncating a name to three letters conveys less than the
   *  avatar already does. The menu itself is unchanged. */
  rail?: boolean
}) {
  const active = agents.find((a) => a.id === activeId)

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={active ? `Workspace: ${active.name}` : "Select an agent"}
        className={cn(
          "flex h-8 items-center gap-2 rounded-lg px-1.5 text-left transition-colors outline-none",
          // Exact same hover as a threads-sidebar row (T676): soft card lift.
          "hover:card-shadow hover:bg-card",
        )}
      >
        {/* Single-line trigger: just the workspace name (the folder path was
            redundant noise — it lives in the menu rows + Finder). The fixed
            `h-8` locks the trigger to the exact height of the sibling
            Threads/Finder/Cockpit view-toggle pill group (also `h-8`).
            In `rail` mode only the glyph survives — see the prop's doc. */}
        <TriggerFace active={active} rail={rail} />
      </DropdownMenuTrigger>

      <DropdownMenuContent
        className="w-[280px]"
        align="start"
        side={rail ? "right" : "bottom"}
        sideOffset={6}
      >
        {onManageAgents && (
          <>
            <DropdownMenuItem
              onClick={onManageAgents}
              className={cn(
                "flex items-center gap-2.5 py-1.5 font-medium",
                "focus:bg-[color-mix(in_oklab,var(--signal)_11%,transparent)]! focus:text-foreground!",
                "data-highlighted:bg-[color-mix(in_oklab,var(--signal)_11%,transparent)]! data-highlighted:text-foreground!",
              )}
            >
              <span className="flex size-7 shrink-0 items-center justify-center text-(--signal)">
                <LayoutGrid className="size-3.5" />
              </span>
              <span className="text-[12.5px] text-foreground/90 group-focus/dropdown-menu-item:text-foreground! group-data-highlighted/dropdown-menu-item:text-foreground!">
                Manage Agents
              </span>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
          </>
        )}
        <DropdownMenuGroup>
          {agents
            .filter((a) => a.id !== activeId)
            .toSorted((a, b) => statusOrder[a.status] - statusOrder[b.status])
            .map((a) => (
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
                  {/* base-ui's DropdownMenuItem ships a `focus:**:text-accent-foreground`
                    rule that recolours EVERY descendant on highlight — in light
                    mode accent-foreground is near-white, so the name turned
                    white-on-white over our soft signal wash (T302). The item-level
                    `!text-foreground` override doesn't reach this grandchild span, so
                    we pin the name to `foreground` on BOTH highlight signals
                    (data-highlighted + DOM focus) via the parent's
                    `group/dropdown-menu-item` — a scoped child rule that outranks the
                    universal `**:` descendant rule and leaves the status label + ✓
                    (their own colours) untouched. Correct contrast in both themes. */}
                  <span className="truncate text-[12.5px] font-medium text-foreground/90 group-focus/dropdown-menu-item:text-foreground! group-data-highlighted/dropdown-menu-item:text-foreground!">
                    {a.name}
                  </span>
                </div>
                <span
                  className="shrink-0 text-[10px] font-medium"
                  style={{ color: statusMeta[a.status].color }}
                >
                  {statusMeta[a.status].label}
                </span>
              </DropdownMenuItem>
            ))}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** The switcher trigger's contents: agent glyph, workspace name, chevron. In
 *  `rail` mode the name and chevron are dropped — the 56px rail has no room for
 *  either, and the avatar already identifies the workspace. Extracted from
 *  {@link AgentSwitcher} so that component stays within the 150-line budget. */
function TriggerFace({ active, rail }: { active: Agent | undefined; rail: boolean }) {
  return (
    <>
      {active ? (
        <>
          <AgentDot
            accent={active.accent}
            status={active.status}
            agentId={active.id}
            hasAvatar={active.hasAvatar}
            compact
          />
          {!rail && (
            <span className="truncate text-[12.5px] font-semibold text-foreground/90">
              {active.name}
            </span>
          )}
        </>
      ) : (
        <>
          <PlaceholderDot />
          {!rail && (
            <span className="truncate text-[12.5px] font-semibold text-foreground/80">
              Select an agent
            </span>
          )}
        </>
      )}
      {!rail && <ChevronDown className="ml-0.5 size-3.5 shrink-0 text-muted-foreground/50" />}
    </>
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
