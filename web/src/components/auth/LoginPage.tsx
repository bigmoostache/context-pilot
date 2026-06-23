// ── Login / bootstrap-register page (Phase 9) ───────────────────────
//
// Shown by AuthGuard when auth is enabled but no valid session exists.
// Two modes driven by `bootstrapped` from the auth context:
//   • Bootstrap (no users yet) → "Create Admin Account" register form.
//   • Normal → "Sign In" login form.

import { useState, type FormEvent } from "react"
import { useAuth } from "@/lib/support/auth"

export function LoginPage() {
  const { login, register, bootstrapped, loading: authLoading } = useAuth()

  const [email, setEmail] = useState("")
  const [name, setName] = useState("")
  const [password, setPassword] = useState("")
  const [error, setError] = useState("")
  const [submitting, setSubmitting] = useState(false)

  const isRegister = !bootstrapped

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setError("")
    setSubmitting(true)
    try {
      if (isRegister) {
        await register(email, name, password)
      } else {
        await login(email, password)
      }
    } catch (err) {
      const msg =
        err instanceof Error ? err.message : "An unexpected error occurred"
      // Extract the human-readable part after the status code.
      const clean = msg.replace(/^\d+\s+\/api\/auth\/\w+:\s*/, "")
      setError(clean || msg)
    } finally {
      setSubmitting(false)
    }
  }

  if (authLoading) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background">
        <div className="text-muted-foreground animate-pulse">Loading…</div>
      </div>
    )
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
            {isRegister
              ? "Create the admin account to get started"
              : "Sign in to continue"}
          </p>
        </div>

        {/* ── Form card ───────────────────────────────────────── */}
        <form
          onSubmit={handleSubmit}
          className="rounded-lg border border-border bg-card p-6 shadow-md"
        >
          {/* Email */}
          <label className="mb-4 block">
            <span className="mb-1 block text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Email
            </span>
            <input
              type="email"
              required
              autoFocus
              autoComplete="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground
                         placeholder:text-muted-foreground/50
                         focus:border-signal focus:outline-none focus:ring-1 focus:ring-signal"
              placeholder="you@company.com"
            />
          </label>

          {/* Name (register only) */}
          {isRegister && (
            <label className="mb-4 block">
              <span className="mb-1 block text-xs font-medium uppercase tracking-wider text-muted-foreground">
                Name
              </span>
              <input
                type="text"
                required
                autoComplete="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground
                           placeholder:text-muted-foreground/50
                           focus:border-signal focus:outline-none focus:ring-1 focus:ring-signal"
                placeholder="Your name"
              />
            </label>
          )}

          {/* Password */}
          <label className="mb-5 block">
            <span className="mb-1 block text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Password
            </span>
            <input
              type="password"
              required
              minLength={8}
              autoComplete={isRegister ? "new-password" : "current-password"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground
                         placeholder:text-muted-foreground/50
                         focus:border-signal focus:outline-none focus:ring-1 focus:ring-signal"
              placeholder={isRegister ? "Min 8 characters" : "••••••••"}
            />
          </label>

          {/* Error */}
          {error && (
            <div className="mb-4 rounded-md bg-danger/10 px-3 py-2 text-xs text-danger">
              {error}
            </div>
          )}

          {/* Submit */}
          <button
            type="submit"
            disabled={submitting}
            className="w-full rounded-md bg-signal px-4 py-2 text-sm font-semibold text-background
                       transition-opacity hover:opacity-90
                       disabled:cursor-not-allowed disabled:opacity-50"
          >
            {submitting
              ? "…"
              : isRegister
                ? "Create Admin Account"
                : "Sign In"}
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
