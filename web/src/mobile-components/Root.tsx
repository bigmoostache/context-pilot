import { useState, useCallback } from "react"

import { FleetDashboard } from "@/mobile-components/agents/FleetDashboard"
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
  return (
    <ThemeProvider>
      <AuthProvider>
        <AccountProvider>
          <DevModeProvider>
            <ShowOverlayProvider>
              <TooltipProvider delay={350} closeDelay={80}>
                <AuthGuard>
                  <MobileShell />
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

  // Any non-fleet view needs a live agent; a stale/empty selection falls back to
  // the fleet grid (mirrors desktop's effectiveView guard).
  const effectiveView: MobileView = view !== "fleet" && !activeAgent ? "fleet" : view

  const body = () => {
    if (effectiveView === "fleet") {
      return (
        <FleetDashboard
          agents={agents}
          onOpenAgent={openAgent}
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
          contextually (fleet → open agent → threads; show-in-finder → finder). */}
      <div className="min-h-0 flex-1 overflow-auto">{body()}</div>
    </div>
  )
}

export default Root
