import { useState } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { LeftRail } from "@/components/shell/LeftRail"
import { Conversation } from "@/components/conversation/Conversation"
import { RightInspector } from "@/components/shell/RightInspector"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { AgentsView } from "@/components/agents/AgentsView"
import { ThemeProvider } from "@/lib/theme"
import { activeAgentId as initialAgentId } from "@/lib/mock"
import type { ViewMode } from "@/lib/types"
import "./App.css"

function App() {
  const [view, setView] = useState<ViewMode>("agents")
  const [activeAgentId, setActiveAgentId] = useState(initialAgentId)

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
        {view === "agents" ? (
          <AgentsView onOpenAgent={openAgent} />
        ) : view === "cockpit" ? (
          <div className="flex min-h-0 flex-1">
            <LeftRail />
            <Conversation />
            <RightInspector />
          </div>
        ) : (
          <ThreadsView
            activeAgentId={activeAgentId}
            onOpenCockpit={() => setView("cockpit")}
          />
        )}
        <StatusBar />
      </div>
    </ThemeProvider>
  )
}

export default App
