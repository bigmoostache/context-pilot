import { useState } from "react"
import { Activity, LayoutGrid, MessagesSquare, FolderTree, Home, Settings2 } from "lucide-react"
import { fmtCost } from "@/lib/panelMeta"
import { ThemeToggle } from "./ThemeToggle"
import { AgentSwitcher } from "./AgentSwitcher"
import { ConfigModal } from "./ConfigModal"
import { ProfileModal } from "./ProfileModal"
import { StatsPopup } from "./StatsPopup"
import { UserMenu } from "./UserMenu"
import { AgentModal } from "@/components/agents/AgentModal"
import { Tip } from "@/components/ui/tip"
import type { Agent, ViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"

interface TopBarProps {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  activeAgentId: string
  onSwitchAgent: (id: string) => void
  /** Raise a "create new agent" request (→ fleet agents page + dialog). */
  onNewAgent: () => void
  agents: Agent[]
}

/** Slim macOS-style title bar — app mark (→ fleet), workspace switcher,
 *  per-agent view tabs (Threads · Cockpit · Finder), branch, cost, theme. */
export function TopBar({ view, onViewChange, activeAgentId, onSwitchAgent, onNewAgent, agents }: TopBarProps) {
  const activeAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]
  const inFleet = view === "fleet"
  const [configOpen, setConfigOpen] = useState(false)
  const [statsOpen, setStatsOpen] = useState(false)
  const [manageOpen, setManageOpen] = useState(false)
  const [profileOpen, setProfileOpen] = useState(false)

  return (
    <>
    <header className="vibrancy flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
      {/* app mark → fleet dashboard (mission control) */}
      <Tip title="Mission control" body="Back to the fleet — an overview of all your agents." side="bottom">
        <button
          onClick={() => onViewChange("fleet")}
          className={cn(
            "flex items-center gap-1.5 rounded-md px-1.5 py-1 transition-colors",
            inFleet ? "text-foreground" : "text-foreground/90 hover:bg-muted/50",
          )}
        >
          <Home className="size-4 text-[var(--signal)]" />
          <span className="text-[13px] font-semibold tracking-tight">Context Pilot</span>
        </button>
      </Tip>

      {/* Workspace switcher — always present. Inside an agent it shows the
          active workspace; at fleet altitude (no agent focused) it falls back
          to a neutral "Select an agent" placeholder so the card never vanishes.
          Picking an agent here enters it (→ threads view). */}
      <span className="ml-1 text-muted-foreground/40">/</span>
      <AgentSwitcher
        agents={agents}
        activeId={inFleet ? undefined : activeAgentId}
        onSwitch={
          inFleet
            ? (id) => {
                onSwitchAgent(id)
                onViewChange("threads")
              }
            : onSwitchAgent
        }
        onFleet={() => onViewChange("fleet")}
        onNewAgent={onNewAgent}
      />

      {/* per-agent view switcher (hidden at fleet altitude). Order: Threads ·
          Finder · Cockpit (T25). Each tab carries a tooltip explaining the view
          since the names aren't obvious to a first-time user. */}
      {!inFleet && (
        <div className="ml-2 flex items-center gap-0.5 rounded-lg border border-border bg-muted/60 p-0.5">
          <Tip
            title="Threads"
            body="Chat with this agent. Each thread is a separate conversation or task it can run in parallel."
          >
            <ViewTab
              active={view === "threads"}
              onClick={() => onViewChange("threads")}
              icon={MessagesSquare}
              label="Threads"
            />
          </Tip>
          <Tip
            title="Finder"
            body="Browse this agent's files — the project folder it lives in and is confined to."
          >
            <ViewTab
              active={view === "finder"}
              onClick={() => onViewChange("finder")}
              icon={FolderTree}
              label="Finder"
            />
          </Tip>
          <Tip
            title="Cockpit"
            body="Look inside the agent's mind: its live context panels — memory, todos, stats and more."
          >
            <ViewTab
              active={view === "cockpit"}
              onClick={() => onViewChange("cockpit")}
              icon={LayoutGrid}
              label="Cockpit"
            />
          </Tip>
        </div>
      )}

      <div className="ml-auto flex items-center gap-3">
        {/* cost is agent-scoped — only meaningful inside an agent */}
        {!inFleet && (
          <span className="text-[12px] tabular-nums text-muted-foreground">
            {fmtCost(activeAgent?.costUsd ?? 0)}
          </span>
        )}
        {/* session vitals are agent-scoped — irrelevant at fleet altitude */}
        {!inFleet && (
          <Tip title="Session vitals" body="Live tokens, cost and context-budget for this agent." side="bottom">
            <button
              onClick={() => setStatsOpen(true)}
              className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground"
              aria-label="Open session stats"
            >
              <Activity className="size-[17px]" />
            </button>
          </Tip>
        )}
        <Tip title="Appearance" body="Switch between light and dark." side="bottom">
          <span className="inline-flex">
            <ThemeToggle />
          </span>
        </Tip>
        <span className="h-5 w-px bg-border/70" />
        {/* per-agent configuration — the one-click shortcut to the same
            "Manage <agent>" dialog the fleet exposes, so editing the focused
            agent no longer needs the four-step switcher journey (T26). Only
            meaningful inside an agent; sits just left of the global gear so the
            two cog buttons (agent-scoped vs global) read as a pair. */}
        {!inFleet && (
          <Tip
            title="Agent configuration"
            body="Rename, switch model, or archive this agent — the same dialog as Manage."
            side="bottom"
          >
            <button
              onClick={() => setManageOpen(true)}
              className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground"
              aria-label="Configure this agent"
            >
              <Settings2 className="size-[17px]" />
            </button>
          </Tip>
        )}
        {/* Account avatar menu (T30) — replaced the old top-right Settings
            gear. The gear's behaviour is preserved: the menu's "Settings" item
            still opens the same ConfigModal, and "Profile" opens the profile
            sheet. To revert, restore a <Tip><button onClick={() =>
            setConfigOpen(true)}><Settings/></button></Tip> here in place of
            <UserMenu/> (and re-import the Settings icon). */}
        <UserMenu
          onOpenSettings={() => setConfigOpen(true)}
          onOpenProfile={() => setProfileOpen(true)}
        />
      </div>

      <ConfigModal open={configOpen} onClose={() => setConfigOpen(false)} />
      <ProfileModal open={profileOpen} onClose={() => setProfileOpen(false)} />
      <StatsPopup open={statsOpen} onClose={() => setStatsOpen(false)} />
    </header>

    {/* Rendered as a SIBLING of the .vibrancy header (never a descendant) so its
        `absolute inset-0` backdrop anchors to the viewport and escapes the
        header's backdrop-filter containing block. */}
    {!inFleet && manageOpen && activeAgent && (
      <AgentModal
        modal={{ mode: "manage", agent: activeAgent }}
        onClose={() => setManageOpen(false)}
      />
    )}
    </>
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
