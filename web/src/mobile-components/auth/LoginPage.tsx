// ── Login / bootstrap-register page — mobile twin ───────────────────
//
// Touch twin of the desktop LoginPage. Same two modes driven by `bootstrapped`
// (bootstrap-register when no users exist, else sign-in) and the same submit
// logic — only the presentation is mobile-tuned: a full-width card, ≥44px touch
// controls, and 16px inputs so focusing a field never triggers iOS Safari's
// focus-zoom.

import { useState, type SyntheticEvent } from "react"
import { useAuth } from "@/lib/providers/auth"

export function LoginPage() {
  const { login, register, bootstrapped, loading: authLoading } = useAuth()

  const [email, setEmail] = useState("")
  const [name, setName] = useState("")
  const [password, setPassword] = useState("")
  const [error, setError] = useState("")
  const [submitting, setSubmitting] = useState(false)

  if (authLoading) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background">
        <div className="animate-pulse text-muted-foreground">Loading…</div>
      </div>
    )
  }

  // Bootstrap mode (no users yet) → the register form; otherwise sign-in.
  // Declared after the loading early-exit so the guard path doesn't compute it
  // (unicorn/no-declarations-before-early-exit) — every use is in the JSX below.
  const isRegister = !bootstrapped

  // Defined after the early exit for the same reason; the submit path picks
  // login vs register off `bootstrapped` (positive test — no negated condition).
  const handleSubmit = async (e: SyntheticEvent) => {
    e.preventDefault()
    setError("")
    setSubmitting(true)
    try {
      if (bootstrapped) {
        await login(email, password)
      } else {
        await register(email, name, password)
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : "An unexpected error occurred"
      // Extract the human-readable part after the status code.
      const clean = msg.replace(/^\d+\s+\/api\/auth\/\w+:\s*/, "")
      setError(clean || msg)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background p-4">
      <div className="w-full max-w-sm">
        {/* ── Branding ────────────────────────────────────────── */}
        <div className="mb-8 text-center">
          <div className="mb-2 font-mono text-2xl font-bold tracking-tight text-foreground">
            <span className="text-signal">▌</span> Context Pilot
          </div>
          <p className="text-sm text-muted-foreground">
            {isRegister ? "Create the admin account to get started" : "Sign in to continue"}
          </p>
        </div>

        {/* ── Form card ───────────────────────────────────────── */}
        <form
          onSubmit={(e) => void handleSubmit(e)}
          className="rounded-lg border border-border bg-card p-6 shadow-md"
        >
          {/* Email */}
          <label className="mb-4 block">
            <span className="mb-1 block text-xs font-medium tracking-wider text-muted-foreground uppercase">
              Email
            </span>
            <input
              type="email"
              required
              autoFocus
              autoComplete="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              className="w-full rounded-md border border-border bg-background p-3 text-base text-foreground
                         placeholder:text-muted-foreground/50
                         focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
              placeholder="you@company.com"
            />
          </label>

          {/* Name (register only) */}
          {isRegister && (
            <label className="mb-4 block">
              <span className="mb-1 block text-xs font-medium tracking-wider text-muted-foreground uppercase">
                Name
              </span>
              <input
                type="text"
                required
                autoComplete="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="w-full rounded-md border border-border bg-background p-3 text-base text-foreground
                           placeholder:text-muted-foreground/50
                           focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
                placeholder="Your name"
              />
            </label>
          )}

          {/* Password */}
          <label className="mb-5 block">
            <span className="mb-1 block text-xs font-medium tracking-wider text-muted-foreground uppercase">
              Password
            </span>
            <input
              type="password"
              required
              minLength={8}
              autoComplete={isRegister ? "new-password" : "current-password"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="w-full rounded-md border border-border bg-background p-3 text-base text-foreground
                         placeholder:text-muted-foreground/50
                         focus:border-signal focus:ring-1 focus:ring-signal focus:outline-none"
              placeholder={isRegister ? "Min 8 characters" : "••••••••"}
            />
          </label>

          {/* Error */}
          {error && (
            <div
              role="alert"
              aria-live="assertive"
              className="mb-4 rounded-md bg-danger/10 px-3 py-2 text-xs text-danger"
            >
              {error}
            </div>
          )}

          {/* Submit */}
          <button
            type="submit"
            disabled={submitting}
            className="w-full rounded-md bg-signal px-4 py-3 text-base font-semibold text-background
                       transition-opacity hover:opacity-90
                       disabled:cursor-not-allowed disabled:opacity-50"
          >
            {submitting ? "…" : isRegister ? "Create Admin Account" : "Sign In"}
          </button>
        </form>

        {/* ── Footer ──────────────────────────────────────────── */}
        <p className="mt-4 text-center text-xs text-muted-foreground/60">
          {isRegister
            ? "This will be the system administrator account."
            : "Contact your admin if you need an account."}
        </p>
      </div>
    </div>
  )
}
