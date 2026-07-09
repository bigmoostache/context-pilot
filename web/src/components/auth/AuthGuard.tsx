// ── Auth guard — conditional rendering based on auth state (Phase 9) ─
//
// Wraps the main application shell. Outcomes:
//   • Auth disabled → render children immediately (zero overhead, NFR-09).
//   • Auth enabled, no session → render LoginPage.
//   • Auth enabled → the backend's `next_action` (on the /me profile) decides
//     between password rotation, first-run onboarding, and the app itself.
//   • Still probing → full-screen loading indicator.
//
// The post-login step is server-driven: AuthGuard renders whatever
// `user.next_action` says, never re-deriving the flow client-side. The
// password-change and onboarding screens call `refreshMe()` on success, which
// re-pulls /me and advances `next_action`.

import { type ReactNode } from "react"
import { useAuth } from "@/lib/providers/auth"
import { LoginPage } from "./LoginPage"
import { Onboarding } from "./Onboarding"
import { ForcePasswordChange } from "./ForcePasswordChange"
import { DayZeroSetup } from "./DayZeroSetup"

export function AuthGuard({ children }: { children: ReactNode }) {
  const { authEnabled, user, loading, refreshMe } = useAuth()

  // Still checking backend status / validating token.
  if (loading || authEnabled === null) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background">
        <div className="animate-pulse font-mono text-sm text-muted-foreground">
          <span className="text-signal">▌</span> Connecting…
        </div>
      </div>
    )
  }

  // Auth disabled — pass through.
  if (!authEnabled) return <>{children}</>

  // Auth enabled but no valid session — show login.
  if (!user) return <LoginPage />

  // Backend-driven post-login step.
  switch (user.next_action) {
    case "change_password": {
      return <ForcePasswordChange />
    }
    case "set_identity": {
      // Day-0 (design §13.4): an IT-capable operator names the unprovisioned box
      // (which brings :443 up) and distributes the CA root, replacing the removed
      // maintenance wizard.
      return <DayZeroSetup />
    }
    case "onboarding": {
      return <Onboarding onComplete={() => void refreshMe()} />
    }
    default: {
      return <>{children}</>
    }
  }
}
