import { useState, useCallback, useEffect } from "react"

import { FleetDashboard } from "@/mobile-components/agents/FleetDashboard"
import { AgentModal } from "@/mobile-components/agents/AgentModal"
import { ThreadsView } from "@/mobile-components/threads/ThreadsView"
import { Finder } from "@/mobile-components/finder/Finder"
import { TooltipProvider } from "@/mobile-components/ui/tooltip"
import { AuthGuard } from "@/mobile-components/auth/AuthGuard"
import { ThemeProvider } from "@/lib/providers/ThemeProvider"
import { AccountProvider } from "@/lib/providers/AccountProvider"
import { AuthProvider } from "@/lib/providers/AuthProvider"
import { DevModeProvider } from "@/lib/providers/toggles/DevModeProvider"
import { ShowOverlayProvider } from "@/lib/providers/toggles/ShowOverlayProvider"
import { useFleet } from "@/lib/live"
import { TopButtonsProvider } from "@/lib/providers/topButtons/provider"
import { useTopButtons } from "@/lib/providers/topButtons"
import type { ViewMode } from "@/lib/types"
import "@/App.css"

/**
 * Mobile component-tree root — the FIRST divergent (hand-authored, marker-less)
 * twin of `components/Root`, and the proof that the mirror + switch machinery
 * works end-to-end (design §8 P4).
 *
 * It is a **provider-contract boundary** (design §11.8): it mounts the SAME
 * global contexts its desktop twin does — theme, auth, account, dev-mode,
 * overlay toggles, and the tooltip layer — so any shared child that consumes one
 * of those contexts behaves identically on either tree. The providers come from
 * the shared `@/lib` layer (not forked); only the presentation children resolve
 * through the `@/mobile-components` token, which is what the leak guard enforces.
 *
 * Ancestor-promotion in action (design §3.3): this real `Root` is a promoted
 * ancestor that routes every view child through `@/mobile-components/…` (e.g. the
 * divergent `threads/ThreadsView` leaf) — so a future divergence anywhere beneath
 * it is reachable, never bypassed by a stub.
 */
function Root() {
  // Tag <html> as the mobile tree so mobile-only theme overrides in index.css
  // (`.dark.mobile` → true-black background) apply. This can't be a CSS media
  // query: the desktop/mobile split is a frozen JS matchMedia probe (width OR
  // `pointer: coarse`), which a `max-width` query would desync from. Cleared on
  // unmount for safety, though the tree is chosen once per session.
  useEffect(() => {
    document.documentElement.classList.add("mobile")
    return () => document.documentElement.classList.remove("mobile")
  }, [])

  return (
    <ThemeProvider>
      <AuthProvider>
        <AccountProvider>
          <DevModeProvider>
            <ShowOverlayProvider>
              <TooltipProvider delay={350} closeDelay={80}>
                <AuthGuard>
                  <TopButtonsProvider>
                    <MobileShell />
                  </TopButtonsProvider>
                </AuthGuard>
              </TooltipProvider>
            </ShowOverlayProvider>
          </DevModeProvider>
        </AccountProvider>
      </AuthProvider>
    </ThemeProvider>
  )
}

/** The three primary mobile surfaces — the desktop `costs` view is a dev-only
 *  analytics surface and is intentionally omitted from the mobile shell (P4
 *  proof-of-concept scope). */
type MobileView = Extract<ViewMode, "fleet" | "threads" | "finder">

/**
 * Mobile app shell — the chrome that diverges from desktop `AppShell`.
 *
 * Same view-routing model as desktop (fleet → threads/finder for a selected
 * agent, persisted to the same `cp-view` / `cp-agent` localStorage keys so the
 * two trees agree on last-view across a reload). The mobile chrome is currently
 * minimal to the point of absence: there is no top bar and no bottom tab bar
 * (both removed, T611) — persistent navigation is being reworked, so for now the
 * views are reached contextually (fleet → open agent → threads; show-in-finder
 * → finder).
 *
 * The disconnect-overlay + live-vitals plumbing desktop `AppShell` carries is
 * elided here for the P4 proof-of-concept: views receive a non-disconnected,
 * no-op reconnect contract. Wiring the live SSE vitals into the mobile shell is
 * follow-up work, not part of proving the mirror mechanism.
 */
function MobileShell() {
  const { data: agents = [] } = useFleet()
  const [view, setView] = useState<MobileView>(() => {
    const stored = localStorage.getItem("cp-view")
    return stored === "threads" || stored === "finder" ? stored : "fleet"
  })
  const [activeAgentId, setActiveAgentId] = useState(() => localStorage.getItem("cp-agent") ?? "")
  const [finderRevealPath, setFinderRevealPath] = useState<string | null>(null)
  // Which agent's full-screen Settings page is open (null = none). It's a
  // transient overlay, NOT a persisted `view` — a reload must never land on it,
  // so it lives in its own state rather than the cp-view union. The origin the
  // back button returns to is stashed in localStorage (`cameToAgentSettingsFrom`)
  // per the T636 spec: "agentsList" or the agent id (opened from that thread).
  const [settingsAgentId, setSettingsAgentId] = useState<string | null>(null)

  const activeAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]

  // Persist view + agent selection (write-through, same keys as desktop).
  const changeView = useCallback((next: MobileView) => {
    setView(next)
    localStorage.setItem("cp-view", next)
  }, [])
  const openAgent = useCallback(
    (id: string) => {
      setActiveAgentId(id)
      localStorage.setItem("cp-agent", id)
      changeView("threads")
    },
    [changeView],
  )
  const showInFinder = useCallback(
    (path: string) => {
      setFinderRevealPath(path)
      changeView("finder")
    },
    [changeView],
  )

  // Open the agent Settings page, stashing where Back should return: "agentsList"
  // when opened from the fleet grid, or the agent id when opened from that
  // agent's thread page (so Back re-selects that agent's threads). T636 spec.
  const openAgentSettings = useCallback((agentId: string, fromList: boolean) => {
    localStorage.setItem("cameToAgentSettingsFrom", fromList ? "agentsList" : agentId)
    setSettingsAgentId(agentId)
  }, [])

  // Close the Settings page and return to origin (read from localStorage): the
  // fleet grid, or that agent's threads view.
  const backFromSettings = useCallback(() => {
    const from = localStorage.getItem("cameToAgentSettingsFrom")
    setSettingsAgentId(null)
    if (from && from !== "agentsList") {
      setActiveAgentId(from)
      localStorage.setItem("cp-agent", from)
      changeView("threads")
    } else {
      changeView("fleet")
    }
  }, [changeView])

  // Any non-fleet view needs a live agent; a stale/empty selection falls back to
  // the fleet grid (mirrors desktop's effectiveView guard).
  const effectiveView: MobileView = view !== "fleet" && !activeAgent ? "fleet" : view

  // Signal the top-buttons provider on every page switch so the corner controls
  // re-spring their glyph in lock-step with the transition (T637). Fires on
  // mount too, seeding the entrance spring.
  const { bump } = useTopButtons()
  useEffect(bump, [effectiveView, bump])

  // The agent whose Settings page is open, resolved to a live fleet member (a
  // just-retired agent vanishes from the list → the overlay closes itself).
  const settingsAgent = settingsAgentId ? agents.find((a) => a.id === settingsAgentId) : undefined

  const body = () => {
    if (effectiveView === "fleet") {
      return (
        <FleetDashboard
          agents={agents}
          onOpenAgent={openAgent}
          onManageAgent={(id) => openAgentSettings(id, true)}
          autoCreate={false}
          onAutoCreateConsumed={() => {
            /* mobile PoC: auto-create dialog not wired */
          }}
        />
      )
    }
    if (effectiveView === "finder" && activeAgent) {
      return (
        <Finder
          key={activeAgent.id}
          agent={activeAgent}
          revealPath={finderRevealPath}
          onRevealConsumed={() => setFinderRevealPath(null)}
          disconnected={false}
          onReconnect={() => {
            /* mobile PoC: live reconnect not wired */
          }}
        />
      )
    }
    return (
      <ThreadsView
        key={activeAgentId}
        activeAgentId={activeAgentId}
        onShowInFinder={showInFinder}
        onGoToAgents={() => changeView("fleet")}
        onOpenSettings={activeAgent ? () => openAgentSettings(activeAgent.id, false) : undefined}
        disconnected={false}
        onReconnect={() => {
          /* mobile PoC: live reconnect not wired */
        }}
      />
    )
  }

  return (
    // h-dvh (dynamic viewport height), NOT h-screen (100vh): on iOS Safari
    // 100vh is TALLER than the visible viewport (it counts the space under the
    // address bar), so an h-screen shell pushes its bottom-anchored composer
    // below the fold — it only reappears when the page is scrolled, which is
    // exactly the "composer disappears when I scroll the thread" bug (T617).
    // h-dvh tracks the *visible* viewport, so the composer sits at the real
    // bottom of the screen and stays put.
    <div className="flex h-dvh w-screen flex-col overflow-hidden bg-background text-foreground">
      {/* No persistent chrome on mobile — no top bar, no bottom tab bar (T611).
          View navigation is being reworked; for now views are reached
          contextually (fleet → open agent → threads; show-in-finder → finder).

          pt safe-area inset: when the app runs as a standalone home-screen web
          app (apple-mobile-web-app-status-bar-style=black-translucent) the web
          view extends UNDER the iOS status bar, so the scrollable content is
          padded down so it never sits beneath the clock/battery. Only the TOP
          is padded here — the BOTTOM (home-indicator) inset is owned by each
          view's own bottom-anchored element (e.g. the composer's
          pb-[max(1rem,env(safe-area-inset-bottom))]), so padding it here too
          would double-count. In a plain Safari tab the inset is 0 = no-op. */}
      {/* Fixed-height flex column (NOT overflow-auto): each view is a
          `flex-1 min-h-0 flex-col` root that owns its OWN scroll (ThreadsView's
          conversation ScrollArea, the fleet/finder ScrollAreas). A scrolling
          outer wrapper here was a SECOND scroll layer — it let a view grow to
          its content height, so ThreadConversation's `absolute bottom-0`
          composer pinned to the bottom of ALL the messages (below the fold) and
          scrolled with them instead of staying on-screen (T637). As a fixed
          flex column with `overflow-hidden`, the view fills the viewport exactly,
          its inner ScrollArea is the sole scroller, and the floating composer
          pins to the real bottom of the screen.

          NO top safe-area padding here (T639): a shared `pt-[env(safe-area-
          inset-top)]` on this wrapper pushes every view's scroll VIEWPORT below
          the iOS status bar, so content clips at the viewport top and never
          scrolls UNDER the translucent bar (the "threads doesn't use the top
          space" bug). Instead each scrolling view reaches y=0 and pads its own
          scroll CONTENT with env(safe-area-inset-top) — the pattern the Agent
          Settings page already uses (fixed inset-0 + internal header pad) — so
          content sits below the clock at rest but scrolls edge-to-edge under it.
          The agent-settings overlay owns its inset itself (fixed inset-0). */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{body()}</div>

      {/* Agent Settings page — a full-screen (fixed inset-0) overlay above every
          view. Rendered only when open AND the target agent still exists; its own
          back chevron calls backFromSettings to return to the stashed origin. */}
      {settingsAgent && <AgentModal agent={settingsAgent} onClose={backFromSettings} />}
    </div>
  )
}

export default Root
