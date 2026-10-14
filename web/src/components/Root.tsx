import { useState, useCallback, useEffect, useRef } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { CostsView } from "@/components/shell/costs/CostsView"
import { StatusBar } from "@/components/shell/StatusBar"
import { AgentsView } from "@/components/agents/AgentsView"
import { FleetDashboard } from "@/components/fleets/FleetDashboard"
import { FleetModal } from "@/components/fleets/FleetModal"
import { Finder } from "@/components/finder/Finder"
import { TooltipProvider } from "@/components/ui/tooltip"
import { AuthGuard } from "@/components/auth/AuthGuard"
import { ThemeProvider } from "@/lib/providers/ThemeProvider"
import { AccountProvider } from "@/lib/providers/AccountProvider"
import { AuthProvider } from "@/lib/providers/AuthProvider"
import { FeaturesProvider } from "@/lib/providers/toggles/FeaturesProvider"
import { DevModeProvider } from "@/lib/providers/toggles/DevModeProvider"
import { ShowOverlayProvider } from "@/lib/providers/toggles/ShowOverlayProvider"
import { AsideDefaultProvider } from "@/lib/providers/toggles/AsideDefaultProvider"
import { useDevMode } from "@/lib/providers/toggles/devMode"
import { useFleet, useFleetMeta, useSseConnected, useRestartFlow } from "@/lib/live"
import { TelemetryProfiler } from "@/lib/support/telemetry"
import { TelemetryHud } from "@/components/shell/widgets/TelemetryHud"
import type { ViewMode, Fleet } from "@/lib/types"
import "@/App.css"

/**
 * Desktop component-tree root — the mirror twin of `mobile-components/Root`.
 * Mounts the global contexts (theme, auth, flags, account, dev-mode) + the
 * tooltip layer above {@link AppShell}; AuthGuard drives login + `next_action`
 * (§13.4). Provider-contract boundary (§11.8): mobile `Root` mounts the same set.
 */
function Root() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <FeaturesProvider>
          <AccountProvider>
            <DevModeProvider>
              <ShowOverlayProvider>
                <AsideDefaultProvider>
                  <TooltipProvider delay={350} closeDelay={80}>
                    <AuthGuard>
                      <AppShell />
                    </AuthGuard>
                  </TooltipProvider>
                </AsideDefaultProvider>
              </ShowOverlayProvider>
            </DevModeProvider>
          </AccountProvider>
        </FeaturesProvider>
      </AuthProvider>
    </ThemeProvider>
  )
}

/** The navigation atoms browser Back / Next restore (T636). One degree finer
 *  than view+agent: the accessed THREAD (agents view) rides along, so Back
 *  steps through the pages actually visited.
 *  Coarser below that (finder tab/split stays in localStorage; rail toggles are
 *  never entries). `agentId` is optional — only the field relevant to
 *  `view` is recorded, and an entry lacking it decodes fine. */
interface NavState {
  view: ViewMode
  fleetId: string
  agentId?: string | undefined
}

/** Narrow the `any`-typed `history.state` / `PopStateEvent.state` down to a
 *  {@link NavState}, containing the `any` at this one boundary so no unsafe
 *  access leaks into the hook. A shape that isn't ours (a foreign history entry,
 *  or none) yields undefined. `view` is asserted after the string check — an
 *  unknown value just falls through the view router's fleet fallback, so it
 *  needs no exhaustive membership test. */
function readNav(state: unknown): NavState | undefined {
  if (typeof state !== "object" || state === null) return undefined
  const nav: unknown = (state as { nav?: unknown }).nav
  if (typeof nav !== "object" || nav === null) return undefined
  const { view, fleetId, agentId } = nav as {
    view?: unknown
    fleetId?: unknown
    agentId?: unknown
  }
  if (typeof view !== "string" || typeof fleetId !== "string") return undefined
  return {
    view: view as ViewMode,
    fleetId,
    agentId: typeof agentId === "string" ? agentId : undefined,
  }
}

/** Whether two nav entries point at the same surface — the dedupe test that
 *  keeps a redundant push (which would make one Back a no-op) off the stack. */
function sameNav(a: NavState, b: NavState): boolean {
  return a.view === b.view && a.fleetId === b.fleetId && a.agentId === b.agentId
}

/** localStorage key holding a fleet's last-selected agent. Module-scoped (no
 *  closure over shell state) so it seeds the state initializer, the persist
 *  effect, and the nav-restore fallback from one place. */
const agentKeyFor = (id: string) => `cp-agent-sel-${id}`

/**
 * Wire browser Back / Next to the (view, agent, agent) tuple via
 * the History API directly — the app has no router and needs none here.
 *
 * Three effects: (1) PUSH on change — each tuple change pushes a state-only
 * entry, except a popstate echo (loop guard) or a push matching the top entry;
 * the first run `replaceState`s so the app starts with no duplicate base entry.
 * (2) APPLY on popstate — hands the entry's `nav` to `apply`, setting the atoms.
 * (3) LOOP GUARD — `applyingRef` swallows the one push-effect run that applying
 * a popstate triggers, so the entry just navigated to isn't re-pushed (same
 * one-shot-ref trick the SSE reducers use).
 *
 * URLs stay untouched: a hard refresh falls back to the localStorage-persisted
 * view + agent, the pre-existing behaviour.
 */
function useNavHistory(
  current: {
    view: ViewMode
    fleetId: string
    agentId: string | undefined
  },
  apply: (s: NavState) => void,
) {
  const { view, fleetId, agentId } = current
  const applyingRef = useRef(false)
  const bootRef = useRef(false)

  useEffect(() => {
    // A popstate echo: the atoms changed because we just applied a history
    // entry, so there is nothing new to record. Swallow exactly one run.
    if (applyingRef.current) {
      applyingRef.current = false
      return
    }
    // Record only the atom relevant to the current view: a agents entry
    // carries its agent and every other view carries none, so a stale agent
    // never leaks into a settings/finder entry (which would break both the
    // dedupe test and the restore).
    const nav: NavState = {
      view,
      fleetId,
      ...(view === "agents" && agentId && { agentId }),
    }
    if (!bootRef.current) {
      // Seed the current location as the base entry rather than pushing a
      // second one on top of it.
      bootRef.current = true
      history.replaceState({ nav }, "")
      return
    }
    const cur = readNav(history.state)
    if (cur && sameNav(cur, nav)) return
    history.pushState({ nav }, "")
  }, [view, fleetId, agentId])

  useEffect(() => {
    const onPop = (e: PopStateEvent) => {
      const nav = readNav(e.state)
      if (!nav) return
      // Mark the coming push-effect run as a popstate echo (mechanism 3).
      applyingRef.current = true
      apply(nav)
    }
    window.addEventListener("popstate", onPop)
    return () => window.removeEventListener("popstate", onPop)
  }, [apply])
}

/** Props for {@link ShellViews} — the shell's view atoms + the handlers each
 *  surface needs. Extracted verbatim from AppShell's former `renderView` so the
 *  container stays under the per-function line budget; purely a presentational
 *  router, all behaviour lives in the handlers passed down. */
interface ShellViewsProps {
  effectiveView: ViewMode
  fleets: Fleet[]
  activeFleet: ReturnType<typeof useFleetMeta>["data"] | undefined
  activeFleetId: string
  openFleet: (id: string) => void
  showInFinder: (path: string) => void
  disconnected: boolean
  onReconnect: () => void
  agentsRailOpen: boolean
  selectedAgentId: string
  onAgentChange: (id: string) => void
  newAgentOpen: boolean
  onNewOpenChange: (v: boolean) => void
  agentSearchOpen: boolean
  onSearchOpenChange: (v: boolean) => void
  finderRailOpen: boolean
  finderRevealPath: string | null
  onRevealConsumed: () => void
}

/** Route the active view to its surface. A flat if-chain (not a nested ternary)
 *  so each branch reads cleanly and the fleet fallthrough is explicit. */
function ShellViews(p: ShellViewsProps) {
  if (p.effectiveView === "fleet") {
    return <FleetDashboard fleets={p.fleets} onOpenFleet={p.openFleet} />
  }
  if (p.effectiveView === "costs") {
    return (
      <CostsView
        fleetId={p.activeFleetId}
        disconnected={p.disconnected}
        onReconnect={p.onReconnect}
      />
    )
  }
  if (p.effectiveView === "finder" && p.activeFleet) {
    return (
      <Finder
        key={p.activeFleet.id}
        fleet={p.activeFleet}
        railOpen={p.finderRailOpen}
        revealPath={p.finderRevealPath}
        onRevealConsumed={p.onRevealConsumed}
        disconnected={p.disconnected}
        onReconnect={p.onReconnect}
      />
    )
  }
  return (
    <AgentsView
      key={p.activeFleetId}
      activeFleetId={p.activeFleetId}
      selectedAgentId={p.selectedAgentId}
      onAgentChange={p.onAgentChange}
      onShowInFinder={p.showInFinder}
      railOpen={p.agentsRailOpen}
      newOpen={p.newAgentOpen}
      onNewOpenChange={p.onNewOpenChange}
      searchOpen={p.agentSearchOpen}
      onSearchOpenChange={p.onSearchOpenChange}
      disconnected={p.disconnected}
      onReconnect={p.onReconnect}
    />
  )
}

/** The agent-settings DIALOG (T760) — a command, not a destination. Extracted
 *  from {@link AppShell} so the open/agent guards cost the shell no complexity
 *  budget: a dialog is dismissed, never routed to, so it takes no history entry
 *  and lives in its own flag rather than in {@link NavState}. */
function FleetSettingsDialog({
  open,
  fleet,
  onClose,
}: {
  open: boolean
  fleet: ShellViewsProps["activeFleet"]
  onClose: () => void
}) {
  if (!open || !fleet) return null
  return <FleetModal modal={{ mode: "manage", fleet }} onClose={onClose} />
}

function AppShell() {
  const { devMode } = useDevMode()
  const { data: fleets = [] } = useFleet()
  const [view, setView] = useState<ViewMode>(() => {
    const modes: Record<string, ViewMode> = {
      fleet: "fleet",
      agents: "agents",
      threads: "agents", // pre-T860 persisted value
      finder: "finder",
      costs: "costs",
    }
    return modes[localStorage.getItem("cp-view") ?? ""] ?? "fleet"
  })
  const [activeFleetId, setActiveFleetId] = useState(() => localStorage.getItem("cp-agent") ?? "")

  // The accessed THREAD — one degree finer than view+agent — is OWNED HERE so
  // browser Back/Next can step through it (T636). The
  // agent selection used to live inside AgentsView's useAgentSelection; it is
  // lifted so a history entry can carry it and a popstate can restore it. Agent
  // selection stays persisted per agent under the SAME `cp-agent-<id>` key the
  // hook used, so a reload still returns to the last agent; the shell is now
  // simply the single writer of that key (the hook defers to it when controlled).
  const [selectedAgentId, setSelectedAgentId] = useState(
    () => localStorage.getItem(agentKeyFor(activeFleetId)) ?? "",
  )

  // Persist the selected agent per agent (single writer — see above).
  useEffect(() => {
    if (selectedAgentId) localStorage.setItem(agentKeyFor(activeFleetId), selectedAgentId)
  }, [activeFleetId, selectedAgentId])

  // Persist view + agent selection across reloads (write-through effects rather
  // than setter wrappers, so the useState setters keep their canonical names).
  useEffect(() => {
    localStorage.setItem("cp-view", view)
  }, [view])
  useEffect(() => {
    localStorage.setItem("cp-agent", activeFleetId)
  }, [activeFleetId])

  // Identity + roster come from the polled fleet list; the LIVE vitals (phase,
  // cost, tokens, status) come from the per-agent meta cache, which the SSE
  // bridge folds in real time (T297). Spreading the delta-folded meta over the
  // fleet row makes the always-visible TopBar + StatusBar reactive instead of
  // riding the 15s fleet poll — the same gold path agents already use.
  const fleetAgent = fleets.find((a) => a.id === activeFleetId) ?? fleets[0]
  const { data: liveFleet, loading: fleetLoading } = useFleetMeta(activeFleetId)
  const activeFleet = liveFleet ?? fleetAgent
  const sseConnected = useSseConnected(activeFleetId)
  const { restart: restartFleet, restarting: fleetRestarting } = useRestartFlow(activeFleetId)

  // A persisted view of "agents"/"finder" requires a live agent to
  // render. If the fleet is still loading, or the stored agent id no longer
  // matches any live agent (stale localStorage — e.g. the agent was removed),
  // `activeFleet` is undefined and those views would crash on `activeFleet.id`.
  // Fall back to the fleet view in that case (private windows never hit this
  // because they start with empty localStorage → default "fleet").
  //
  // Costs is a DEVELOPER-only surface (T301): when dev mode is off,
  // a persisted (or stale) selection resolves to "agents" so the view can
  // never render a tab the TopBar deliberately hides.
  const effectiveView: ViewMode =
    view === "costs" && !devMode
      ? activeFleet
        ? "agents"
        : "fleet"
      : view !== "fleet" && !activeFleet
        ? "fleet"
        : view

  // Open an agent → drop into its agents. Switching agent from the fleet
  // dashboard is the ONLY place an agent is chosen/managed. Load that agent's
  // last-selected agent synchronously (a direct localStorage read, not a
  // seed effect — an effect would race a nav restore that sets the agent
  // explicitly).
  const openFleet = (id: string) => {
    setActiveFleetId(id)
    setSelectedAgentId(localStorage.getItem(agentKeyFor(id)) ?? "")
    setView("agents")
  }

  // Whether the agents view's list rail is open. It lives HERE, not in
  // AgentsView, because the control that toggles it is the header rail's
  // Agents tab — a sibling of the view, not a descendant. AppShell is the
  // nearest common ancestor, and it already owns the other cross-cutting shell
  // state (view, active agent, reveal path).
  //
  // Side effect of lifting it: the rail no longer resets when you switch agent
  // (AgentsView is keyed by agent id, AppShell is not), so a collapsed rail
  // stays collapsed across the switch. Deliberate — it is panel state, not
  // per-agent state. Not persisted to localStorage.
  const [agentsRailOpen, setAgentsRailOpen] = useState(true)

  // The finder view's explorer rail — its OWN flag, same reasoning as above.
  // Re-clicking the Finder tab while finder is active flips it (the activity-bar
  // idiom the Agents tab already uses).
  const [finderRailOpen, setFinderRailOpen] = useState(true)

  // The two agent-list ACTIONS, hoisted for the same reason as the rail above:
  // their buttons now live in the header rail, a sibling of the agents view.
  // Only the flags live here — the New Agent dialog and the search palette
  // both still render down in the view, where the agents + agent they operate
  // on are in scope.
  const [newAgentOpen, setNewAgentOpen] = useState(false)
  const [agentSearchOpen, setAgentSearchOpen] = useState(false)

  // The agent-settings DIALOG (T760). Restored after T617/T757 turned it into a
  // routed view: it is a command, not a destination, so it must never enter a
  // history entry — a dialog is dismissed, not navigated back from.
  // The flag lives HERE because the rail button that opens it is a SIBLING of the
  // surfaces it configures, and AppShell is their nearest common ancestor.
  const [manageOpen, setManageOpen] = useState(false)

  // T334: "Show in Finder" — switch to finder view and reveal a specific file.
  const [finderRevealPath, setFinderRevealPath] = useState<string | null>(null)
  const showInFinder = useCallback((path: string) => {
    setFinderRevealPath(path)
    // A reveal that lands on a collapsed rail would hide the very row it just
    // expanded to — so force the explorer open on the way in.
    setFinderRailOpen(true)
    setView("finder")
  }, [])

  // Browser Back / Next restore the (view, agent, agent) tuple (T636).
  // Declared after `effectiveView` so the entry we record is the surface the
  // user actually sees (the gated one), not the raw pre-gate `view`. `applyNav`
  // sets the raw atoms and falls back to the agent's persisted agent (a
  // settings/finder entry carries no agentId), so a restore never blanks a
  // field the entry simply didn't record. Setters are stable, so the callback
  // never changes identity.
  const applyNav = useCallback((s: NavState) => {
    setView(s.view)
    setActiveFleetId(s.fleetId)
    setSelectedAgentId(s.agentId ?? localStorage.getItem(agentKeyFor(s.fleetId)) ?? "")
  }, [])
  useNavHistory(
    {
      view: effectiveView,
      fleetId: activeFleetId,
      agentId: selectedAgentId || undefined,
    },
    applyNav,
  )

  // When the agent is unreachable (SSE down OR registry-stale) and we're
  // viewing an agent surface, blur+grey the main content and intercept all
  // clicks to trigger reconnect.
  const fleetStale = activeFleet?.status === "disconnected"
  // Suppress the blur+grey overlay during an active restart — the spinner
  // already communicates the transition, and flashing "Disconnected" during a
  // controlled restart is visual noise, not a genuine failure signal.
  const showDisconnectOverlay =
    (!sseConnected || fleetStale) && effectiveView !== "fleet" && !fleetRestarting

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
    <div className="relative flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      <div className="flex min-h-0 flex-1 flex-row overflow-hidden">
        <TopBar
          view={effectiveView}
          onViewChange={setView}
          activeFleetId={activeFleetId}
          onSwitchFleet={setActiveFleetId}
          fleets={fleets}
          agentsRailOpen={agentsRailOpen}
          onToggleAgentsRail={() => setAgentsRailOpen((o) => !o)}
          onNewAgent={() => setNewAgentOpen(true)}
          onSearchAgents={() => setAgentSearchOpen(true)}
          onManageFleet={() => setManageOpen(true)}
          finderRailOpen={finderRailOpen}
          onToggleFinderRail={() => setFinderRailOpen((o) => !o)}
        />

        {/* The view's own box, unchanged: every view roots itself on
            `flex min-h-0 flex-1`, so it still resolves against a COLUMN. */}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <TelemetryProfiler id={effectiveView}>
            <ShellViews
              effectiveView={effectiveView}
              fleets={fleets}
              activeFleet={activeFleet}
              activeFleetId={activeFleetId}
              openFleet={openFleet}
              showInFinder={showInFinder}
              disconnected={showDisconnectOverlay}
              onReconnect={restartFleet}
              agentsRailOpen={agentsRailOpen}
              selectedAgentId={selectedAgentId}
              onAgentChange={setSelectedAgentId}
              newAgentOpen={newAgentOpen}
              onNewOpenChange={setNewAgentOpen}
              agentSearchOpen={agentSearchOpen}
              onSearchOpenChange={setAgentSearchOpen}
              finderRailOpen={finderRailOpen}
              finderRevealPath={finderRevealPath}
              onRevealConsumed={() => setFinderRevealPath(null)}
            />
          </TelemetryProfiler>
        </div>
      </div>

      <StatusBar
        fleet={effectiveView === "fleet"}
        fleets={fleets}
        activeFleet={activeFleet}
        activeFleetId={activeFleetId}
        connected={sseConnected && !fleetStale}
        onRestart={restartFleet}
        restarting={fleetRestarting}
        loading={fleetLoading}
      />

      {/* The agent-settings DIALOG (T760). Sibling of the row above, never a
          descendant of the header rail: the backdrop is `absolute inset-0` and
          must anchor to the VIEWPORT, not to a rail containing block. The
          button that opens it lives in that rail, the state that flags it here
          — a dialog is dismissed, never routed to, so it takes no history
          entry. */}
      <FleetSettingsDialog
        open={manageOpen}
        fleet={activeFleet}
        onClose={() => setManageOpen(false)}
      />

      {/* Dev-mode performance HUD (gated on the Developer-mode flag inside).
          `fixed`-positioned, so where it sits in the tree is immaterial. */}
      <TelemetryHud />
    </div>
  )
}

export default Root
