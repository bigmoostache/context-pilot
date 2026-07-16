import { useState, useCallback, useEffect } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { CostsView } from "@/components/shell/costs/CostsView"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { FleetDashboard } from "@/components/agents/FleetDashboard"
import { Finder } from "@/components/finder/Finder"
import { TooltipProvider } from "@/components/ui/tooltip"
import { AuthGuard } from "@/components/auth/AuthGuard"
import { ThemeProvider } from "@/lib/providers/ThemeProvider"
import { AccountProvider } from "@/lib/providers/AccountProvider"
import { AuthProvider } from "@/lib/providers/AuthProvider"
import { DevModeProvider } from "@/lib/providers/toggles/DevModeProvider"
import { ShowOverlayProvider } from "@/lib/providers/toggles/ShowOverlayProvider"
import { useDevMode } from "@/lib/providers/toggles/devMode"
import { useFleet, useAgentMeta, useSseConnected, useRestartFlow } from "@/lib/live"
import { TelemetryProfiler } from "@/lib/support/telemetry"
import { TelemetryHud } from "@/components/shell/widgets/TelemetryHud"
import type { ViewMode } from "@/lib/types"
import "./App.css"

/**
 * Root provider shell. Mounts the global contexts (theme, auth, account,
 * dev-mode) and the tooltip layer **above** {@link AppShell}. AuthProvider
 * probes the backend's auth status on mount; AuthGuard shows the login page
 * when auth is enabled but no valid session exists, and drives the backend's
 * `next_action` post-login flow — including the day-0 provisioning steps that
 * used to live on the removed maintenance plane (design §13.4).
 */
function App() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <AccountProvider>
          <DevModeProvider>
            <ShowOverlayProvider>
              <TooltipProvider delay={350} closeDelay={80}>
                <AuthGuard>
                  <AppShell />
                </AuthGuard>
              </TooltipProvider>
            </ShowOverlayProvider>
          </DevModeProvider>
        </AccountProvider>
      </AuthProvider>
    </ThemeProvider>
  )
}

function AppShell() {
  const { devMode } = useDevMode()
  const { data: agents = [] } = useFleet()
  const [view, setView] = useState<ViewMode>(() => {
    const modes: Record<string, ViewMode> = { fleet: "fleet", threads: "threads", finder: "finder", costs: "costs" }
    return modes[localStorage.getItem("cp-view") ?? ""] ?? "fleet"
  })
  const [activeAgentId, setActiveAgentId] = useState(() => localStorage.getItem("cp-agent") ?? "")
  // One-shot request to pop the "create agent" dialog on the fleet dashboard
  // (raised by the workspace switcher's "New agent" entry).
  const [createAgent, setCreateAgent] = useState(false)

  // Persist view + agent selection across reloads (write-through effects rather
  // than setter wrappers, so the useState setters keep their canonical names).
  useEffect(() => {
    localStorage.setItem("cp-view", view)
  }, [view])
  useEffect(() => {
    localStorage.setItem("cp-agent", activeAgentId)
  }, [activeAgentId])

  // Identity + roster come from the polled fleet list; the LIVE vitals (phase,
  // cost, tokens, status) come from the per-agent meta cache, which the SSE
  // bridge folds in real time (T297). Spreading the delta-folded meta over the
  // fleet row makes the always-visible TopBar + StatusBar reactive instead of
  // riding the 15s fleet poll — the same gold path threads already use.
  const fleetAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]
  const { data: liveAgent, loading: agentLoading } = useAgentMeta(activeAgentId)
  const activeAgent = liveAgent ?? fleetAgent
  const sseConnected = useSseConnected(activeAgentId)
  const { restart: restartAgent, restarting: agentRestarting } = useRestartFlow(activeAgentId)

  // A persisted view of "threads"/"finder" requires a live agent to
  // render. If the fleet is still loading, or the stored agent id no longer
  // matches any live agent (stale localStorage — e.g. the agent was removed),
  // `activeAgent` is undefined and those views would crash on `activeAgent.id`.
  // Fall back to the fleet view in that case (private windows never hit this
  // because they start with empty localStorage → default "fleet").
  //
  // Costs is a DEVELOPER-only surface (T301): when dev mode is off,
  // a persisted (or stale) selection resolves to "threads" so the view can
  // never render a tab the TopBar deliberately hides.
  const effectiveView: ViewMode =
    view === "costs" && !devMode
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

  // Route the active view to its surface. A flat if-chain (not a nested ternary)
  // so each branch reads cleanly and the fleet fallthrough is explicit.
  const renderView = () => {
    if (effectiveView === "fleet") {
      return (
        <FleetDashboard
          agents={agents}
          onOpenAgent={openAgent}
          autoCreate={createAgent}
          onAutoCreateConsumed={() => setCreateAgent(false)}
        />
      )
    }
    if (effectiveView === "costs") {
      return <CostsView agentId={activeAgentId} disconnected={showDisconnectOverlay} onReconnect={restartAgent} />
    }
    if (effectiveView === "finder" && activeAgent) {
      return (
        <Finder
          key={activeAgent.id}
          agent={activeAgent}
          revealPath={finderRevealPath}
          onRevealConsumed={() => setFinderRevealPath(null)}
          disconnected={showDisconnectOverlay}
          onReconnect={restartAgent}
        />
      )
    }
    return (
      <ThreadsView
        key={activeAgentId}
        activeAgentId={activeAgentId}
        onShowInFinder={showInFinder}
        disconnected={showDisconnectOverlay}
        onReconnect={restartAgent}
      />
    )
  }

  // When the agent is unreachable (SSE down) and we're viewing an agent surface,
  // blur+grey the main content and intercept all clicks to trigger reconnect.
  const showDisconnectOverlay = !sseConnected && effectiveView !== "fleet"

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

      <TelemetryProfiler id={effectiveView}>{renderView()}</TelemetryProfiler>

      <StatusBar
        fleet={effectiveView === "fleet"}
        agents={agents}
        activeAgent={activeAgent}
        connected={sseConnected}
        onRestart={restartAgent}
        restarting={agentRestarting}
        loading={agentLoading}
      />

      {/* Dev-mode performance HUD (gated on the Developer-mode flag inside). */}
      <TelemetryHud />
    </div>
  )
}

export default App
