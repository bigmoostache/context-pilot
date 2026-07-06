// ── First-run onboarding (admin) ─────────────────────────────────────
//
// Shown by AuthGuard right after first admin login, while the central
// `onboarding_completed` flag is still false. Provider API keys are
// provisioned out-of-band by the operator (vendor, over SSH/Ansible) — the
// client never enters them — and each user picks their own model per agent, so
// there is nothing for the admin to configure here. Onboarding is just a
// first-run acknowledgement that flips `onboarding_completed`.

import { useState, type FormEvent } from "react"
import { updateSettings } from "@/lib/api"

export function Onboarding({ onComplete }: { onComplete: () => void }) {
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (busy) return
    setError(null)
    setBusy(true)
    try {
      await updateSettings({ onboarding_completed: true })
      onComplete()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Onboarding failed")
      setBusy(false)
    }
  }

  return (
    <div className="flex min-h-screen w-screen items-center justify-center overflow-auto bg-background p-4">
      <div className="w-full max-w-lg py-8">
        {/* ── Branding ────────────────────────────────────────── */}
        <div className="mb-8 text-center">
          <div className="mb-2 font-mono text-2xl font-bold tracking-tight text-foreground">
            <span className="text-signal">▌</span> Context Pilot
          </div>
          <p className="text-sm text-muted-foreground">
            This device is ready. Your users pick their own model per agent; you can restrict which
            models are available later in Settings.
          </p>
        </div>

        <form
          onSubmit={submit}
          className="flex flex-col gap-6 rounded-lg border border-border bg-card p-6 shadow-md"
        >
          {error && (
            <div className="rounded-md bg-danger/10 px-3 py-2 text-xs text-danger">{error}</div>
          )}

          <button
            type="submit"
            disabled={busy}
            className="w-full rounded-md bg-signal px-4 py-2 text-sm font-semibold text-background
                       transition-opacity hover:opacity-90
                       disabled:cursor-not-allowed disabled:opacity-50"
          >
            {busy ? "Finishing…" : "Finish setup"}
          </button>
        </form>
      </div>
    </div>
  )
}
