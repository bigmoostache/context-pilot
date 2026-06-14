import { TopBar } from "@/components/shell/TopBar"
import { LeftRail } from "@/components/shell/LeftRail"
import { Conversation } from "@/components/conversation/Conversation"
import { RightInspector } from "@/components/shell/RightInspector"
import { StatusBar } from "@/components/shell/StatusBar"
import "./App.css"

function App() {
  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      <TopBar />
      <div className="flex min-h-0 flex-1">
        <LeftRail />
        <Conversation />
        <RightInspector />
      </div>
      <StatusBar />
    </div>
  )
}

export default App
