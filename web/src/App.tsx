import { useState } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { CockpitView } from "@/components/shell/CockpitView"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { FleetShell } from "@/components/agents/FleetShell"
import { Finder } from "@/components/finder/Finder"
import { TooltipProvider } from "@/components/ui/tooltip"
import { ThemeProvider } from "@/lib/theme"
import { AccountProvider } from "@/lib/account"
import { useFleet } from "@/lib/live"
import type { ViewMode } from "@/lib/types"
import "./App.css"

function App() {
  const { data: agents = [] } = useFleet()
  const [view, setViewRaw] = useState<ViewMode>(
    () => (localStorage.getItem("cp-view") as ViewMode) ?? "fleet",
  )
  const [activeAgentId, setActiveAgentIdRaw] = useState(
    () => localStorage.getItem("cp-agent") ?? "",
  )
  // One-shot request to pop the "create agent" dialog on the fleet dashboard
  // (raised by the workspace switcher's "New agent" entry).
  const [createAgent, setCreateAgent] = useState(false)

  // Persist view + agent selection across reloads.
  const setView = (v: ViewMode) => {
    setViewRaw(v)
    localStorage.setItem("cp-view", v)
  }
  const setActiveAgentId = (id: string) => {
    setActiveAgentIdRaw(id)
    localStorage.setItem("cp-agent", id)
  }

  const activeAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]

  // Open an agent → drop into its threads. Switching agent from the fleet
  // dashboard is the ONLY place an agent is chosen/managed.
  const openAgent = (id: string) => {
    setActiveAgentId(id)
    setView("threads")
  }

  // "New agent" from the switcher → fleet altitude + create dialog.
  const newAgent = () => {
    setView("fleet")
    setCreateAgent(true)
  }

  return (
    <ThemeProvider>
      <AccountProvider>
        <TooltipProvider delay={350} closeDelay={80}>
        <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
          <TopBar
            view={view}
            onViewChange={setView}
            activeAgentId={activeAgentId}
            onSwitchAgent={setActiveAgentId}
            onNewAgent={newAgent}
            agents={agents}
          />

          {view === "fleet" ? (
            <FleetShell
              agents={agents}
              onOpenAgent={openAgent}
              openCreate={createAgent}
              onCreateConsumed={() => setCreateAgent(false)}
            />
          ) : view === "cockpit" ? (
            <CockpitView agentId={activeAgentId} />
          ) : view === "finder" ? (
            <Finder key={activeAgent.id} agent={activeAgent} />
          ) : (
            <ThreadsView key={activeAgentId} activeAgentId={activeAgentId} />
          )}

          <StatusBar fleet={view === "fleet"} agents={agents} activeAgent={activeAgent} />
        </div>
        </TooltipProvider>
      </AccountProvider>
    </ThemeProvider>
  )
}

export default App
