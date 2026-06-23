// ── Auth guard — conditional rendering based on auth state (Phase 9) ─
//
// Wraps the main application shell. Three outcomes:
//   • Auth disabled → render children immediately (zero overhead, NFR-09).
//   • Auth enabled, valid session → render children.
//   • Auth enabled, no session → render LoginPage.
//   • Still probing → full-screen loading indicator.

import { useAuth } from "@/lib/support/auth"
import { LoginPage } from "./LoginPage"
import type { ReactNode } from "react"

export function AuthGuard({ children }: { children: ReactNode }) {
  const { authEnabled, user, loading } = useAuth()

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

  // Authenticated — render the app.
  return <>{children}</>
}
