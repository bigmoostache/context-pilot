import { useState, useCallback, useEffect, useRef } from "react"
import { TopBar } from "@/components/shell/TopBar"
import { CostsView } from "@/components/shell/costs/CostsView"
import { StatusBar } from "@/components/shell/StatusBar"
import { ThreadsView } from "@/components/threads/ThreadsView"
import { FleetDashboard } from "@/components/agents/FleetDashboard"
import { SettingsView } from "@/components/agents/AgentModal/settingsView"
import type { TabId } from "@/components/agents/AgentModal/tabs"
import { Finder } from "@/components/finder/Finder"
import { TooltipProvider } from "@/components/ui/tooltip"
import { AuthGuard } from "@/components/auth/AuthGuard"
import { ThemeProvider } from "@/lib/providers/ThemeProvider"
import { AccountProvider } from "@/lib/providers/AccountProvider"
import { AuthProvider } from "@/lib/providers/AuthProvider"
import { DevModeProvider } from "@/lib/providers/toggles/DevModeProvider"
import { ShowOverlayProvider } from "@/lib/providers/toggles/ShowOverlayProvider"
import { AsideDefaultProvider } from "@/lib/providers/toggles/AsideDefaultProvider"
import { useDevMode } from "@/lib/providers/toggles/devMode"
import { useFleet, useAgentMeta, useSseConnected, useRestartFlow } from "@/lib/live"
import { TelemetryProfiler } from "@/lib/support/telemetry"
import { TelemetryHud } from "@/components/shell/widgets/TelemetryHud"
import type { ViewMode, Agent } from "@/lib/types"
import "@/App.css"

/**
 * Desktop component-tree root — the mirror twin of `mobile-components/Root`.
 *
 * Mounts the global contexts (theme, auth, account, dev-mode) + the tooltip
 * layer above {@link AppShell}. AuthGuard shows the login page and drives the
 * backend `next_action` post-login flow (design §13.4). A **provider-contract
 * boundary** (design §11.8): any divergent mobile `Root` must mount the same
 * providers, or mobile children consuming these contexts break.
 */
function Root() {
  return (
    <ThemeProvider>
      <AuthProvider>
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
      </AuthProvider>
    </ThemeProvider>
  )
}

/** The navigation atoms browser Back / Next restore (T636). One degree finer
 *  than view+agent: the accessed THREAD (threads view) and settings CATEGORY
 *  (settings view) ride along, so Back steps through the pages actually visited.
 *  Coarser below that (finder tab/split stays in localStorage; rail toggles are
 *  never entries). `threadId`/`settingsTab` are optional — only the field
 *  relevant to `view` is recorded, and an entry lacking them decodes fine. */
interface NavState {
  view: ViewMode
  agentId: string
  threadId?: string | undefined
  settingsTab?: TabId | undefined
}

/** Narrow the `any`-typed `history.state` / `PopStateEvent.state` down to a
 *  {@link NavState}, containing the `any` at this one boundary so no unsafe
 *  access leaks into the hook. A shape that isn't ours (a foreign history entry,
 *  or none) yields undefined. `view`/`settingsTab` are asserted after the string
 *  check — an unknown value just falls through the view router's fleet fallback
 *  or the settings default, so neither needs an exhaustive membership test. */
function readNav(state: unknown): NavState | undefined {
  if (typeof state !== "object" || state === null) return undefined
  const nav: unknown = (state as { nav?: unknown }).nav
  if (typeof nav !== "object" || nav === null) return undefined
  const { view, agentId, threadId, settingsTab } = nav as {
    view?: unknown
    agentId?: unknown
    threadId?: unknown
    settingsTab?: unknown
  }
  if (typeof view !== "string" || typeof agentId !== "string") return undefined
  return {
    view: view as ViewMode,
    agentId,
    threadId: typeof threadId === "string" ? threadId : undefined,
    settingsTab: typeof settingsTab === "string" ? (settingsTab as TabId) : undefined,
  }
}

/** Whether two nav entries point at the same surface — the dedupe test that
 *  keeps a redundant push (which would make one Back a no-op) off the stack. */
function sameNav(a: NavState, b: NavState): boolean {
  return (
    a.view === b.view &&
    a.agentId === b.agentId &&
    a.threadId === b.threadId &&
    a.settingsTab === b.settingsTab
  )
}

/** localStorage key holding an agent's last-selected thread. Module-scoped (no
 *  closure over shell state) so it seeds the state initializer, the persist
 *  effect, and the nav-restore fallback from one place. */
const threadKeyFor = (id: string) => `cp-thread-${id}`

/**
 * Wire browser Back / Next to the (view, agent, thread, settings-tab) tuple via
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
    agentId: string
    threadId: string | undefined
    settingsTab: TabId | undefined
  },
  apply: (s: NavState) => void,
) {
  const { view, agentId, threadId, settingsTab } = current
  const applyingRef = useRef(false)
  const bootRef = useRef(false)

  useEffect(() => {
    // A popstate echo: the atoms changed because we just applied a history
    // entry, so there is nothing new to record. Swallow exactly one run.
    if (applyingRef.current) {
      applyingRef.current = false
      return
    }
    // Record only the atom relevant to the current view: a threads entry
    // carries its thread, a settings entry its category, and neither leaks a
    // stale value into the other's entries (which would break dedupe + restore).
    const nav: NavState = {
      view,
      agentId,
      ...(view === "threads" && threadId && { threadId }),
      ...(view === "settings" && { settingsTab }),
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
  }, [view, agentId, threadId, settingsTab])

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
  agents: Agent[]
  activeAgent: ReturnType<typeof useAgentMeta>["data"] | undefined
  activeAgentId: string
  openAgent: (id: string) => void
  showInFinder: (path: string) => void
  disconnected: boolean
  onReconnect: () => void
  threadsRailOpen: boolean
  selectedThreadId: string
  onThreadChange: (id: string) => void
  newThreadOpen: boolean
  onNewOpenChange: (v: boolean) => void
  threadSearchOpen: boolean
  onSearchOpenChange: (v: boolean) => void
  settingsRailOpen: boolean
  settingsTab: TabId
  onSettingsTab: (t: TabId) => void
  finderRailOpen: boolean
  finderRevealPath: string | null
  onRevealConsumed: () => void
}

/** Route the active view to its surface. A flat if-chain (not a nested ternary)
 *  so each branch reads cleanly and the fleet fallthrough is explicit. */
function ShellViews(p: ShellViewsProps) {
  if (p.effectiveView === "fleet") {
    return <FleetDashboard agents={p.agents} onOpenAgent={p.openAgent} />
  }
  if (p.effectiveView === "costs") {
    return (
      <CostsView
        agentId={p.activeAgentId}
        disconnected={p.disconnected}
        onReconnect={p.onReconnect}
      />
    )
  }
  if (p.effectiveView === "settings" && p.activeAgent) {
    return (
      <SettingsView
        key={p.activeAgent.id}
        agent={p.activeAgent}
        railOpen={p.settingsRailOpen}
        tab={p.settingsTab}
        onTab={p.onSettingsTab}
        disconnected={p.disconnected}
        onReconnect={p.onReconnect}
      />
    )
  }
  if (p.effectiveView === "finder" && p.activeAgent) {
    return (
      <Finder
        key={p.activeAgent.id}
        agent={p.activeAgent}
        railOpen={p.finderRailOpen}
        revealPath={p.finderRevealPath}
        onRevealConsumed={p.onRevealConsumed}
        disconnected={p.disconnected}
        onReconnect={p.onReconnect}
      />
    )
  }
  return (
    <ThreadsView
      key={p.activeAgentId}
      activeAgentId={p.activeAgentId}
      selectedThreadId={p.selectedThreadId}
      onThreadChange={p.onThreadChange}
      onShowInFinder={p.showInFinder}
      railOpen={p.threadsRailOpen}
      newOpen={p.newThreadOpen}
      onNewOpenChange={p.onNewOpenChange}
      searchOpen={p.threadSearchOpen}
      onSearchOpenChange={p.onSearchOpenChange}
      disconnected={p.disconnected}
      onReconnect={p.onReconnect}
    />
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

  // The accessed THREAD and settings CATEGORY — one degree finer than view+agent
  // — are OWNED HERE so browser Back/Next can step through them (T636). The
  // thread selection used to live inside ThreadsView's useThreadSelection; it is
  // lifted so a history entry can carry it and a popstate can restore it. Thread
  // selection stays persisted per agent under the SAME `cp-thread-<id>` key the
  // hook used, so a reload still returns to the last thread; the shell is now
  // simply the single writer of that key (the hook defers to it when controlled).
  const [selectedThreadId, setSelectedThreadId] = useState(
    () => localStorage.getItem(threadKeyFor(activeAgentId)) ?? "",
  )
  const [settingsTab, setSettingsTab] = useState<TabId>("llm")

  // Persist the selected thread per agent (single writer — see above).
  useEffect(() => {
    if (selectedThreadId) localStorage.setItem(threadKeyFor(activeAgentId), selectedThreadId)
  }, [activeAgentId, selectedThreadId])

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
  // dashboard is the ONLY place an agent is chosen/managed. Load that agent's
  // last-selected thread synchronously (a direct localStorage read, not a
  // seed effect — an effect would race a nav restore that sets the thread
  // explicitly).
  const openAgent = (id: string) => {
    setActiveAgentId(id)
    setSelectedThreadId(localStorage.getItem(threadKeyFor(id)) ?? "")
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

  // The settings view's category rail — a SEPARATE flag from the threads rail
  // (collapsing one panel must never silently collapse another the user can't
  // see). Owned here because its toggle is the header rail's Settings tab, a
  // sibling of the view, not a descendant.
  const [settingsRailOpen, setSettingsRailOpen] = useState(true)

  // The finder view's explorer rail — its OWN flag, same reasoning as above.
  // Re-clicking the Finder tab while finder is active flips it (the activity-bar
  // idiom the Threads and Settings tabs already use).
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

  // Browser Back / Next restore the (view, agent, thread, settings-category)
  // tuple (T636). Declared after `effectiveView` so the entry we record is the
  // surface the user actually sees (the gated one), not the raw pre-gate `view`.
  // `applyNav` sets the raw atoms; for the finer fields it falls back to the
  // agent's persisted thread (a settings/finder entry carries no threadId) and
  // keeps the current settings tab (a threads entry carries no settingsTab), so
  // a restore never blanks a field the entry simply didn't record. Setters are
  // stable, so the callback never changes identity.
  const applyNav = useCallback((s: NavState) => {
    setView(s.view)
    setActiveAgentId(s.agentId)
    setSelectedThreadId(s.threadId ?? localStorage.getItem(threadKeyFor(s.agentId)) ?? "")
    if (s.settingsTab) setSettingsTab(s.settingsTab)
  }, [])
  useNavHistory(
    {
      view: effectiveView,
      agentId: activeAgentId,
      threadId: selectedThreadId || undefined,
      settingsTab,
    },
    applyNav,
  )

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
          <TelemetryProfiler id={effectiveView}>
            <ShellViews
              effectiveView={effectiveView}
              agents={agents}
              activeAgent={activeAgent}
              activeAgentId={activeAgentId}
              openAgent={openAgent}
              showInFinder={showInFinder}
              disconnected={showDisconnectOverlay}
              onReconnect={restartAgent}
              threadsRailOpen={threadsRailOpen}
              selectedThreadId={selectedThreadId}
              onThreadChange={setSelectedThreadId}
              newThreadOpen={newThreadOpen}
              onNewOpenChange={setNewThreadOpen}
              threadSearchOpen={threadSearchOpen}
              onSearchOpenChange={setThreadSearchOpen}
              settingsRailOpen={settingsRailOpen}
              settingsTab={settingsTab}
              onSettingsTab={setSettingsTab}
              finderRailOpen={finderRailOpen}
              finderRevealPath={finderRevealPath}
              onRevealConsumed={() => setFinderRevealPath(null)}
            />
          </TelemetryProfiler>
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
