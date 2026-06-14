import { useState } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { LeftRail } from "@/components/shell/LeftRail"
import { Conversation } from "@/components/conversation/Conversation"
import { RightInspector } from "@/components/shell/RightInspector"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { ThemeProvider } from "@/lib/theme"
import type { ViewMode } from "@/lib/types"
import "./App.css"

function App() {
  const [view, setView] = useState<ViewMode>("threads")

  return (
    <ThemeProvider>
      <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
        <TopBar view={view} onViewChange={setView} />
        {view === "cockpit" ? (
          <div className="flex min-h-0 flex-1">
            <LeftRail />
            <Conversation />
            <RightInspector />
          </div>
        ) : (
          <ThreadsView onOpenCockpit={() => setView("cockpit")} />
        )}
        <StatusBar />
      </div>
    </ThemeProvider>
  )
}

export default App
