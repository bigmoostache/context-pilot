import { useState, useCallback, useEffect } from "react"
import { MaintWizard } from "@/components/auth/maint/MaintWizard"
import { probeMaintPlane, type MaintStatus } from "@/lib/api/maint"
import { TopBar } from "@/components/shell/TopBar"
import { CockpitView } from "@/components/shell/CockpitView"
import { CostsView } from "@/components/shell/costs/CostsView"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { FleetShell } from "@/components/agents/FleetShell"
import { Finder } from "@/components/finder/Finder"
import { TooltipProvider } from "@/components/ui/tooltip"
import { AuthGuard } from "@/components/auth/AuthGuard"
import { ThemeProvider } from "@/lib/providers/ThemeProvider"
import { AccountProvider } from "@/lib/providers/AccountProvider"
import { AuthProvider } from "@/lib/providers/AuthProvider"
import { DevModeProvider } from "@/lib/providers/DevModeProvider"
import { useDevMode } from "@/lib/providers/devMode"
import { useFleet, useAgentMeta } from "@/lib/live"
import type { ViewMode } from "@/lib/types"
import "./App.css"

/**
 * Root provider shell. Mounts the global contexts (theme, auth, account,
 * dev-mode) and the tooltip layer **above** {@link AppShell}. AuthProvider
 * probes the backend's auth status on mount; AuthGuard shows the login page
 * when auth is enabled but no valid session exists.
 *
 * Before any of that, it probes whether this origin is the **IT maintenance
 * plane** (:9090). The same bundle serves both planes; on the maintenance plane
 * `GET /api/maint/status` answers, and we render the provisioning wizard instead
 * of the cockpit (Milestone 5). On the cockpit that route 404s, so the normal
 * app renders.
 */
function App() {
  const [maint, setMaint] = useState<MaintStatus | null | "loading">("loading")

  useEffect(() => {
    let live = true
    void probeMaintPlane().then((s) => {
      if (live) setMaint(s)
    })
    return () => {
      live = false
    }
  }, [])

  if (maint === "loading") return null
  if (maint) {
    return (
      <ThemeProvider>
        <MaintWizard initialStatus={maint} />
      </ThemeProvider>
    )
  }

  return (
    <ThemeProvider>
      <AuthProvider>
        <AccountProvider>
          <DevModeProvider>
            <TooltipProvider delay={350} closeDelay={80}>
              <AuthGuard>
                <AppShell />
              </AuthGuard>
            </TooltipProvider>
          </DevModeProvider>
        </AccountProvider>
      </AuthProvider>
    </ThemeProvider>
  )
}

function AppShell() {
  const { devMode } = useDevMode()
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

  // Identity + roster come from the polled fleet list; the LIVE vitals (phase,
  // cost, tokens, status) come from the per-agent meta cache, which the SSE
  // bridge folds in real time (T297). Spreading the delta-folded meta over the
  // fleet row makes the always-visible TopBar + StatusBar reactive instead of
  // riding the 15s fleet poll — the same gold path threads already use.
  const fleetAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]
  const { data: liveAgent } = useAgentMeta(activeAgentId)
  const activeAgent = liveAgent ?? fleetAgent

  // A persisted view of "threads"/"cockpit"/"finder" requires a live agent to
  // render. If the fleet is still loading, or the stored agent id no longer
  // matches any live agent (stale localStorage — e.g. the agent was removed),
  // `activeAgent` is undefined and those views would crash on `activeAgent.id`.
  // Fall back to the fleet view in that case (private windows never hit this
  // because they start with empty localStorage → default "fleet").
  //
  // Cockpit and Costs are DEVELOPER-only surfaces (T301): when dev mode is off,
  // a persisted (or stale) selection resolves to "threads" so the view can
  // never render a tab the TopBar deliberately hides.
  const effectiveView: ViewMode =
    (view === "cockpit" || view === "costs") && !devMode
      ? activeAgent
        ? "threads"
        : "fleet"
      : view !== "fleet" && !activeAgent
        ? "fleet"
        : view

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

  // T334: "Show in Finder" — switch to finder view and reveal a specific file.
  const [finderRevealPath, setFinderRevealPath] = useState<string | null>(null)
  const showInFinder = useCallback((path: string) => {
    setFinderRevealPath(path)
    setView("finder")
  }, [])

  return (
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
          ) : effectiveView === "costs" ? (
            <CostsView agentId={activeAgentId} />
          ) : effectiveView === "finder" ? (
            <Finder key={activeAgent.id} agent={activeAgent} revealPath={finderRevealPath} onRevealConsumed={() => setFinderRevealPath(null)} />
          ) : (
            <ThreadsView key={activeAgentId} activeAgentId={activeAgentId} onShowInFinder={showInFinder} />
          )}

          <StatusBar fleet={effectiveView === "fleet"} agents={agents} activeAgent={activeAgent} />
        </div>
  )
}

export default App
