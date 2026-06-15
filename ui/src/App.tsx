import { useState } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { CockpitView } from "@/components/shell/CockpitView"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { FleetShell } from "@/components/agents/FleetShell"
import { Finder } from "@/components/finder/Finder"
import { ThemeProvider } from "@/lib/theme"
import { activeAgentId as initialAgentId, agents } from "@/lib/mock"
import type { ViewMode } from "@/lib/types"
import "./App.css"

function App() {
  const [view, setView] = useState<ViewMode>("fleet")
  const [activeAgentId, setActiveAgentId] = useState(initialAgentId)

  const activeAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]

  // Open an agent → drop into its threads. Switching agent from the fleet
  // dashboard is the ONLY place an agent is chosen/managed.
  const openAgent = (id: string) => {
    setActiveAgentId(id)
    setView("threads")
  }

  return (
    <ThemeProvider>
      <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
        <TopBar
          view={view}
          onViewChange={setView}
          activeAgentId={activeAgentId}
          onSwitchAgent={setActiveAgentId}
        />

        {view === "fleet" ? (
          <FleetShell onOpenAgent={openAgent} />
        ) : view === "cockpit" ? (
          <CockpitView />
        ) : view === "finder" ? (
          <Finder key={activeAgent.id} agent={activeAgent} />
        ) : (
          <ThreadsView key={activeAgentId} activeAgentId={activeAgentId} />
        )}

        <StatusBar />
      </div>
    </ThemeProvider>
  )
}

export default App
