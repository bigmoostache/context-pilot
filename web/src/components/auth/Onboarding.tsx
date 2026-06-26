// ── First-run onboarding (admin) ─────────────────────────────────────
//
// Shown by AuthGuard right after first admin login, while the central
// `onboarding_completed` flag is still false. Provider API keys are
// provisioned out-of-band by the operator (vendor, over SSH/Ansible) — the
// client never enters them — so onboarding only asks the admin to pick the
// default model used for new agents, then flips `onboarding_completed`.

import { useEffect, useState, type FormEvent } from "react"
import { ModelPicker } from "@/components/agents/ModelPicker"
import { useProviders, defaultModel } from "@/lib/support/models"
import { updateSettings } from "@/lib/api"

export function Onboarding({ onComplete }: { onComplete: () => void }) {
  const { data: providers = [] } = useProviders()
  const [provider, setProvider] = useState("")
  const [model, setModel] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  // Seed the initial selection once the provider registry loads.
  useEffect(() => {
    if (provider || providers.length === 0) return
    const p0 = providers[0]
    setProvider(p0.id)
    setModel(defaultModel(providers, p0.id)?.id ?? p0.models[0]?.id ?? "")
  }, [providers, provider])

  const canSubmit = model !== "" && !busy

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setError(null)
    setBusy(true)
    try {
      await updateSettings({
        default_provider: provider,
        default_model: model,
        onboarding_completed: true,
      })
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
            Set up this device — choose the default model for new agents.
          </p>
        </div>

        <form
          onSubmit={submit}
          className="flex flex-col gap-6 rounded-lg border border-border bg-card p-6 shadow-md"
        >
          {/* ── Default model ── */}
          <section className="flex flex-col gap-3">
            <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Default model
              <span className="ml-2 normal-case text-muted-foreground/60">
                Used for new agents
              </span>
            </span>
            <ModelPicker
              providers={providers}
              provider={provider}
              model={model}
              onChange={(p, m) => {
                setProvider(p)
                setModel(m)
              }}
            />
          </section>

          {error && (
            <div className="rounded-md bg-danger/10 px-3 py-2 text-xs text-danger">
              {error}
            </div>
          )}

          <button
            type="submit"
            disabled={!canSubmit}
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
