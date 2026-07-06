// ── Forced password change (first login) ────────────────────────────
//
// Shown by AuthGuard when the logged-in user carries `must_change_password`
// — set on accounts provisioned with an operator-known initial password (the
// boot-time admin seed). Blocks the rest of the app until the user rotates it.
// On success the backend clears the flag; we refresh the profile so the guard
// lets the user through.

import { useState, type FormEvent } from "react"
import { changePassword } from "@/lib/api"
import { useAuth } from "@/lib/providers/auth"

const MIN_PASSWORD_LEN = 8

export function ForcePasswordChange() {
  const { refreshMe } = useAuth()
  const [current, setCurrent] = useState("")
  const [next, setNext] = useState("")
  const [confirm, setConfirm] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const canSubmit =
    current !== "" && next.length >= MIN_PASSWORD_LEN && next === confirm && !busy

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setError(null)
    setBusy(true)
    try {
      await changePassword(current, next)
      await refreshMe()
      // No onComplete — refreshMe clears must_change_password and AuthGuard
      // re-renders into the app (or onboarding).
    } catch (err) {
      setError(err instanceof Error ? err.message : "Password change failed")
      setBusy(false)
    }
  }

  return (
    <div className="flex min-h-screen w-screen items-center justify-center overflow-auto bg-background p-4">
      <div className="w-full max-w-sm py-8">
        <div className="mb-8 text-center">
          <div className="mb-2 font-mono text-2xl font-bold tracking-tight text-foreground">
            <span className="text-signal">▌</span> Context Pilot
          </div>
          <p className="text-sm text-muted-foreground">
            Choose a new password to finish securing your account.
          </p>
        </div>

        <form
          onSubmit={submit}
          className="flex flex-col gap-4 rounded-lg border border-border bg-card p-6 shadow-md"
        >
          <Field label="Current password" value={current} onChange={setCurrent} autoFocus />
          <Field label="New password" value={next} onChange={setNext} hint={`At least ${MIN_PASSWORD_LEN} characters`} />
          <Field label="Confirm new password" value={confirm} onChange={setConfirm} />

          {confirm !== "" && next !== confirm && (
            <p className="-mt-1 text-[11px] text-danger">Passwords don't match.</p>
          )}
          {error && (
            <div className="rounded-md bg-danger/10 px-3 py-2 text-xs text-danger">{error}</div>
          )}

          <button
            type="submit"
            disabled={!canSubmit}
            className="w-full rounded-md bg-signal px-4 py-2 text-sm font-semibold text-background
                       transition-opacity hover:opacity-90
                       disabled:cursor-not-allowed disabled:opacity-50"
          >
            {busy ? "Updating…" : "Set new password"}
          </button>
        </form>
      </div>
    </div>
  )
}

function Field({
  label,
  value,
  onChange,
  hint,
  autoFocus,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  hint?: string
  autoFocus?: boolean
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-foreground/90">
        {label}
        {hint && <span className="ml-2 text-muted-foreground/60">{hint}</span>}
      </span>
      <input
        type="password"
        value={value}
        autoFocus={autoFocus}
        onChange={(e) => onChange(e.target.value)}
        autoComplete="new-password"
        className="rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground
                   focus:border-signal focus:outline-none focus:ring-1 focus:ring-signal"
      />
    </label>
  )
}
