// ── Auth guard — conditional rendering based on auth state (Phase 9) ─
//
// Wraps the main application shell. Outcomes:
//   • Auth disabled → render children immediately (zero overhead, NFR-09).
//   • Auth enabled, no session → render LoginPage.
//   • Auth enabled, admin, onboarding not done → render Onboarding.
//   • Auth enabled, valid session, onboarding done → render children.
//   • Still probing → full-screen loading indicator.

import { useCallback, useEffect, useState, type ReactNode } from "react"
import { useAuth } from "@/lib/support/auth"
import { fetchSettings } from "@/lib/api"
import { LoginPage } from "./LoginPage"
import { Onboarding } from "./Onboarding"

export function AuthGuard({ children }: { children: ReactNode }) {
  const { authEnabled, user, loading } = useAuth()

  // Onboarding gate: null = not yet known, true/false = whether the admin
  // still needs to run first-time setup.
  const [needsOnboarding, setNeedsOnboarding] = useState<boolean | null>(null)

  const probeOnboarding = useCallback(async () => {
    // Only authenticated admins can run (or need) onboarding.
    if (!authEnabled || !user || user.role !== "admin") {
      setNeedsOnboarding(false)
      return
    }
    try {
      const settings = await fetchSettings()
      setNeedsOnboarding(!settings.onboarding_completed)
    } catch {
      // If settings can't be read, don't trap the user behind onboarding.
      setNeedsOnboarding(false)
    }
  }, [authEnabled, user])

  useEffect(() => {
    if (loading || authEnabled === null) return
    void probeOnboarding()
  }, [loading, authEnabled, user, probeOnboarding])

  // Still checking backend status / validating token.
  if (loading || authEnabled === null) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background">
        <div className="text-muted-foreground animate-pulse font-mono text-sm">
          <span className="text-signal">▌</span> Connecting…
        </div>
      </div>
    )
  }

  // Auth disabled — pass through.
  if (!authEnabled) return <>{children}</>

  // Auth enabled but no valid session — show login.
  if (!user) return <LoginPage />

  // Authenticated — wait for the onboarding probe before deciding.
  if (needsOnboarding === null) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background">
        <div className="text-muted-foreground animate-pulse font-mono text-sm">
          <span className="text-signal">▌</span> Loading…
        </div>
      </div>
    )
  }

  // First-run admin setup.
  if (needsOnboarding) {
    return <Onboarding onComplete={() => setNeedsOnboarding(false)} />
  }

  // Authenticated + onboarded — render the app.
  return <>{children}</>
}
