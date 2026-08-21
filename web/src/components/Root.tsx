import { useState, useCallback, useEffect } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { CostsView } from "@/components/shell/costs/CostsView"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { FleetDashboard } from "@/components/agents/FleetDashboard"
import { SettingsView } from "@/components/agents/AgentModal/settingsView"
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
import "@/App.css"

/**
 * Desktop component-tree root — the mirror twin of `mobile-components/Root`.
 *
 * Extracted verbatim from the former `App.tsx` body; `App.tsx` is now the
 * device switch that lazy-loads this tree or its mobile twin (see `App.tsx`).
 * Mounts the global contexts (theme, auth, account, dev-mode) and the tooltip
 * layer **above** {@link AppShell}. AuthProvider probes the backend's auth
 * status on mount; AuthGuard shows the login page when auth is enabled but no
 * valid session exists, and drives the backend's `next_action` post-login flow
 * — including the day-0 provisioning steps that used to live on the removed
 * maintenance plane (design §13.4).
 *
 * This is a **provider-contract boundary** (design §11.8): any divergent mobile
 * `Root` must mount the same providers, or mobile children that consume these
 * contexts break.
 */
function Root() {
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
    const modes: Record<string, ViewMode> = {
      fleet: "fleet",
      threads: "threads",
      finder: "finder",
      costs: "costs",
    }
    return modes[localStorage.getItem("cp-view") ?? ""] ?? "fleet"
  })
  const [activeAgentId, setActiveAgentId] = useState(() => localStorage.getItem("cp-agent") ?? "")

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

  // Whether the threads view's list rail is open. It lives HERE, not in
  // ThreadsView, because the control that toggles it is the header rail's
  // Threads tab — a sibling of the view, not a descendant. AppShell is the
  // nearest common ancestor, and it already owns the other cross-cutting shell
  // state (view, active agent, reveal path).
  //
  // Side effect of lifting it: the rail no longer resets when you switch agent
  // (ThreadsView is keyed by agent id, AppShell is not), so a collapsed rail
  // stays collapsed across the switch. Deliberate — it is panel state, not
  // per-agent state. Not persisted to localStorage.
  const [threadsRailOpen, setThreadsRailOpen] = useState(true)

  // The settings view's category rail. A SEPARATE flag from the threads rail
  // above, not a shared one: collapsing the thread list is a statement about
  // the thread list, and having it silently collapse the settings categories
  // too would make each panel's state depend on a panel the user cannot see.
  // Same ownership reasoning — the control that toggles it is the header
  // rail's Settings tab, a sibling of the view rather than a descendant.
  const [settingsRailOpen, setSettingsRailOpen] = useState(true)

  // The finder view's explorer rail. Its OWN flag, same reasoning as the two
  // rails above: the control that toggles it is the header rail's Finder tab
  // (a sibling of the view), and collapsing one panel must never silently
  // collapse another the user cannot see. Re-clicking the Finder tab while
  // finder is already the active view flips this (the activity-bar idiom the
  // Threads and Settings tabs already use).
  const [finderRailOpen, setFinderRailOpen] = useState(true)

  // The two thread-list ACTIONS, hoisted for the same reason as the rail above:
  // their buttons now live in the header rail, a sibling of the threads view.
  // Only the flags live here — the New Thread dialog and the search palette
  // both still render down in the view, where the threads + agent they operate
  // on are in scope.
  const [newThreadOpen, setNewThreadOpen] = useState(false)
  const [threadSearchOpen, setThreadSearchOpen] = useState(false)

  // T334: "Show in Finder" — switch to finder view and reveal a specific file.
  const [finderRevealPath, setFinderRevealPath] = useState<string | null>(null)
  const showInFinder = useCallback((path: string) => {
    setFinderRevealPath(path)
    // A reveal that lands on a collapsed rail would hide the very row it just
    // expanded to — so force the explorer open on the way in.
    setFinderRailOpen(true)
    setView("finder")
  }, [])

  // Route the active view to its surface. A flat if-chain (not a nested ternary)
  // so each branch reads cleanly and the fleet fallthrough is explicit.
  const renderView = () => {
    if (effectiveView === "fleet") {
      return <FleetDashboard agents={agents} onOpenAgent={openAgent} />
    }
    if (effectiveView === "costs") {
      return (
        <CostsView
          agentId={activeAgentId}
          disconnected={showDisconnectOverlay}
          onReconnect={restartAgent}
        />
      )
    }
    if (effectiveView === "settings" && activeAgent) {
      return (
        <SettingsView
          key={activeAgent.id}
          agent={activeAgent}
          railOpen={settingsRailOpen}
          disconnected={showDisconnectOverlay}
          onReconnect={restartAgent}
        />
      )
    }
    if (effectiveView === "finder" && activeAgent) {
      return (
        <Finder
          key={activeAgent.id}
          agent={activeAgent}
          railOpen={finderRailOpen}
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
        railOpen={threadsRailOpen}
        newOpen={newThreadOpen}
        onNewOpenChange={setNewThreadOpen}
        searchOpen={threadSearchOpen}
        onSearchOpenChange={setThreadSearchOpen}
        disconnected={showDisconnectOverlay}
        onReconnect={restartAgent}
      />
    )
  }

  // When the agent is unreachable (SSE down OR registry-stale) and we're
  // viewing an agent surface, blur+grey the main content and intercept all
  // clicks to trigger reconnect.
  const agentStale = activeAgent?.status === "disconnected"
  // Suppress the blur+grey overlay during an active restart — the spinner
  // already communicates the transition, and flashing "Disconnected" during a
  // controlled restart is visual noise, not a genuine failure signal.
  const showDisconnectOverlay =
    (!sseConnected || agentStale) && effectiveView !== "fleet" && !agentRestarting

  return (
    // Column, not row: the FOOTER is the full-width floor of the window, and
    // everything else sits in the band above it. That band is the row holding
    // the header rail and the view, so the rail's height is the window minus
    // the footer — it stops where the footer starts rather than running past it
    // to the bottom edge.
    //
    // The inner row needs `min-h-0`: a flex item's automatic minimum size is
    // its content, so a tall view would refuse to shrink and would push the
    // footer off the bottom of the screen instead of scrolling internally.
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      <div className="flex min-h-0 flex-1 flex-row overflow-hidden">
        <TopBar
          view={effectiveView}
          onViewChange={setView}
          activeAgentId={activeAgentId}
          onSwitchAgent={setActiveAgentId}
          agents={agents}
          threadsRailOpen={threadsRailOpen}
          onToggleThreadsRail={() => setThreadsRailOpen((o) => !o)}
          onNewThread={() => setNewThreadOpen(true)}
          onSearchThreads={() => setThreadSearchOpen(true)}
          settingsRailOpen={settingsRailOpen}
          onToggleSettingsRail={() => setSettingsRailOpen((o) => !o)}
          finderRailOpen={finderRailOpen}
          onToggleFinderRail={() => setFinderRailOpen((o) => !o)}
        />

        {/* The view's own box, unchanged: every view roots itself on
            `flex min-h-0 flex-1`, so it still resolves against a COLUMN. */}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <TelemetryProfiler id={effectiveView}>{renderView()}</TelemetryProfiler>
        </div>
      </div>

      <StatusBar
        fleet={effectiveView === "fleet"}
        agents={agents}
        activeAgent={activeAgent}
        activeAgentId={activeAgentId}
        connected={sseConnected && !agentStale}
        onRestart={restartAgent}
        restarting={agentRestarting}
        loading={agentLoading}
      />

      {/* Dev-mode performance HUD (gated on the Developer-mode flag inside).
          `fixed`-positioned, so where it sits in the tree is immaterial. */}
      <TelemetryHud />
    </div>
  )
}

export default Root
