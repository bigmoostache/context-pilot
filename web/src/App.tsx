import { useState } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { CockpitView } from "@/components/shell/CockpitView"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { FleetShell } from "@/components/agents/FleetShell"
import { Finder } from "@/components/finder/Finder"
import { TooltipProvider } from "@/components/ui/tooltip"
import { ThemeProvider } from "@/lib/theme"
import { AccountProvider } from "@/lib/support/account"
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

  // A persisted view of "threads"/"cockpit"/"finder" requires a live agent to
  // render. If the fleet is still loading, or the stored agent id no longer
  // matches any live agent (stale localStorage — e.g. the agent was removed),
  // `activeAgent` is undefined and those views would crash on `activeAgent.id`.
  // Fall back to the fleet view in that case (private windows never hit this
  // because they start with empty localStorage → default "fleet").
  const effectiveView: ViewMode =
    view !== "fleet" && !activeAgent ? "fleet" : view

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
            view={effectiveView}
            onViewChange={setView}
            activeAgentId={activeAgentId}
            onSwitchAgent={setActiveAgentId}
            onNewAgent={newAgent}
            agents={agents}
          />

          {effectiveView === "fleet" ? (
            <FleetShell
              agents={agents}
              onOpenAgent={openAgent}
              openCreate={createAgent}
              onCreateConsumed={() => setCreateAgent(false)}
            />
          ) : effectiveView === "cockpit" ? (
            <CockpitView agentId={activeAgentId} />
          ) : effectiveView === "finder" ? (
            <Finder key={activeAgent.id} agent={activeAgent} />
          ) : (
            <ThreadsView key={activeAgentId} activeAgentId={activeAgentId} />
          )}

          <StatusBar fleet={effectiveView === "fleet"} agents={agents} activeAgent={activeAgent} />
        </div>
        </TooltipProvider>
      </AccountProvider>
    </ThemeProvider>
  )
}

export default App
