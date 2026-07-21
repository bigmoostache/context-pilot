// ── First-run onboarding (admin) — mobile twin ──────────────────────
//
// Touch twin of the desktop Onboarding gate. Behaviour is identical (flip the
// central `onboarding_completed` flag, then let the caller refresh /me); only
// the presentation is mobile-tuned: a full-width card, a ≥44px touch submit,
// and — the load-bearing mobile fix — a 16px control so focusing it never
// triggers iOS Safari's focus-zoom.

import { useState, type SyntheticEvent } from "react"
import { updateSettings } from "@/lib/api"

export function Onboarding({ onComplete }: { onComplete: () => void }) {
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async (e: SyntheticEvent) => {
    e.preventDefault()
    if (busy) return
    setError(null)
    setBusy(true)
    try {
      await updateSettings({ onboarding_completed: true })
      // Signal completion (the caller kicks off a /me refresh): fire-and-forget,
      // so any /me hiccup can't leave the screen stuck on "Finishing…". A failure
      // to persist the flag above still surfaces via the catch below.
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
          onSubmit={(e) => void submit(e)}
          className="flex flex-col gap-6 rounded-lg border border-border bg-card p-6 shadow-md"
        >
          {error && (
            <div className="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</div>
          )}

          <button
            type="submit"
            disabled={busy}
            className="w-full rounded-md bg-signal px-4 py-3 text-base font-semibold text-background
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
