import { useState } from "react"
import { MessagesSquare, FolderTree, BarChart3 } from "lucide-react"
import { ThemeToggle } from "./widgets/ThemeToggle"
import { AgentSwitcher } from "./widgets/AgentSwitcher"
import { UsageButton } from "./widgets/UsageButton"
import { ConfigModal } from "./config/ConfigModal"
import { ProfileModal } from "./widgets/ProfileModal"
import { UserMenu } from "./widgets/UserMenu"
import { UsersDialog } from "@/components/auth/UsersDialog"
import { AgentModal } from "@/components/agents/AgentModal"
import { Tip } from "@/components/ui/tip"
import { useDevMode } from "@/lib/providers/toggles/devMode"
import type { Agent, ViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"

interface TopBarProps {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  activeAgentId: string
  onSwitchAgent: (id: string) => void
  agents: Agent[]
}

/** Slim macOS-style title bar — app mark (→ fleet), workspace switcher,
 *  per-agent view tabs (Threads · Finder), branch, cost, theme. */
export function TopBar({ view, onViewChange, activeAgentId, onSwitchAgent, agents }: TopBarProps) {
  const activeAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]
  // OAuth usage/login widget applies ONLY to the OAuth providers (Bearer token
  // via vault "claude_oauth"). The `anthropic` provider authenticates by
  // x-api-key (ANTHROPIC_API_KEY) and has no OAuth login, so it's excluded.
  const isClaudeOAuth =
    activeAgent?.provider === "claudecode" || activeAgent?.provider === "claudecodev2"
  const inFleet = view === "fleet"
  const { devMode } = useDevMode()
  const [configOpen, setConfigOpen] = useState(false)
  const [manageOpen, setManageOpen] = useState(false)
  const [profileOpen, setProfileOpen] = useState(false)
  const [usersOpen, setUsersOpen] = useState(false)

  return (
    <>
      <header className="vibrancy flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
        <AgentSwitcher
          agents={agents}
          activeId={inFleet ? undefined : activeAgentId}
          onManageAgents={() => onViewChange("fleet")}
          onManageAgent={inFleet ? undefined : () => setManageOpen(true)}
          onSwitch={
            inFleet
              ? (id) => {
                  onSwitchAgent(id)
                  onViewChange("threads")
                }
              : onSwitchAgent
          }
        />

        {!inFleet && <ViewTabs view={view} onViewChange={onViewChange} devMode={devMode} />}

        <TopBarActions
          isClaudeOAuth={isClaudeOAuth}
          setConfigOpen={setConfigOpen}
          setProfileOpen={setProfileOpen}
          setUsersOpen={setUsersOpen}
        />
      </header>

      <ConfigModal open={configOpen} onClose={() => setConfigOpen(false)} />
      <ProfileModal open={profileOpen} onClose={() => setProfileOpen(false)} />
      <UsersDialog open={usersOpen} onClose={() => setUsersOpen(false)} />

      {!inFleet && manageOpen && activeAgent && (
        <AgentModal
          modal={{ mode: "manage", agent: activeAgent }}
          onClose={() => setManageOpen(false)}
        />
      )}
    </>
  )
}

/** Right-side controls cluster: theme toggle, agent-config gear, Claude Usage
 *  button, and the account avatar menu. Extracted from {@link TopBar} so both
 *  components stay within the P8 complexity budget. */
function TopBarActions({
  isClaudeOAuth,
  setConfigOpen,
  setProfileOpen,
  setUsersOpen,
}: {
  isClaudeOAuth: boolean
  setConfigOpen: (v: boolean) => void
  setProfileOpen: (v: boolean) => void
  setUsersOpen: (v: boolean) => void
}) {
  return (
    <div className="ml-auto flex items-center gap-3">
      <Tip title="Appearance" body="Switch between light and dark." side="bottom">
        <span className="inline-flex">
          <ThemeToggle />
        </span>
      </Tip>
      <span className="h-5 w-px bg-border/70" />
      {isClaudeOAuth && <UsageButton />}
      <UserMenu
        onOpenSettings={() => setConfigOpen(true)}
        onOpenProfile={() => setProfileOpen(true)}
        onOpenUsers={() => setUsersOpen(true)}
      />
    </div>
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
          ? "card-shadow bg-card text-foreground"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      <Icon className="size-3.5" />
      {label}
    </button>
  )
}

/** Per-agent view switcher (Threads · Finder · Costs). Costs
 *  is dev-mode only. Extracted from {@link TopBar} so its tab cluster + the
 *  `devMode` gate don't count against the bar's complexity budget. Each tab
 *  carries a tooltip since the names aren't obvious to a first-time user. */
function ViewTabs({
  view,
  onViewChange,
  devMode,
}: {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  devMode: boolean
}) {
  return (
    <div className="ml-2 flex h-8 items-center gap-0.5 rounded-lg border border-border bg-muted/60 p-0.5">
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
      {devMode && (
        <Tip
          title="Cost Analysis"
          body="Per-tick cache efficiency, culprit attribution, and spend breakdown charts."
        >
          <ViewTab
            active={view === "costs"}
            onClick={() => onViewChange("costs")}
            icon={BarChart3}
            label="Costs"
          />
        </Tip>
      )}
    </div>
  )
}
