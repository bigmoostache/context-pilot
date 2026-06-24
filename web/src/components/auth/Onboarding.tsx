// ── First-run onboarding (admin) ─────────────────────────────────────
//
// Shown by AuthGuard right after the admin account is created (register),
// while the central `onboarding_completed` flag is still false. It does NOT
// re-ask for email/name/password — those were just collected at register.
// It collects what the appliance needs to run agents:
//   • a default provider + model for new agents (stored server-side), and
//   • ≥1 central provider API key (admin-managed, shared by all agents).
//
// On submit it writes the keys + defaults and flips `onboarding_completed`,
// then calls `onComplete()` so the guard re-checks and reveals the app.

import { useMemo, useState, type FormEvent } from "react"
import { ModelPicker } from "@/components/agents/ModelPicker"
import { PROVIDERS, defaultModel } from "@/lib/support/models"
import { updateProviderKeys, updateSettings } from "@/lib/api"

/** Providers that take an API key, keyed by the backend's canonical name. */
const KEY_PROVIDERS: { id: string; label: string; sample: string }[] = [
  { id: "anthropic", label: "Anthropic", sample: "sk-ant-…" },
  { id: "xai", label: "Grok (xAI)", sample: "xai-…" },
  { id: "groq", label: "Groq", sample: "gsk_…" },
  { id: "deepseek", label: "DeepSeek", sample: "sk-…" },
]

export function Onboarding({ onComplete }: { onComplete: () => void }) {
  const firstProvider = PROVIDERS[0]?.id ?? "anthropic"
  const [provider, setProvider] = useState(firstProvider)
  const [model, setModel] = useState(defaultModel(firstProvider)?.id ?? "")
  const [keys, setKeys] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const setKey = (id: string, value: string) =>
    setKeys((prev) => ({ ...prev, [id]: value }))

  const filledKeys = useMemo(() => {
    const out: Record<string, string> = {}
    for (const [id, val] of Object.entries(keys)) {
      const v = val.trim()
      if (v) out[id] = v
    }
    return out
  }, [keys])

  const hasKey = Object.keys(filledKeys).length > 0
  const canSubmit = model !== "" && hasKey && !busy

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setError(null)
    setBusy(true)
    try {
      if (hasKey) await updateProviderKeys(filledKeys)
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
            Set up this device — choose a default model and add a provider key.
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
              provider={provider}
              model={model}
              onChange={(p, m) => {
                setProvider(p)
                setModel(m)
              }}
            />
          </section>

          {/* ── Provider keys ── */}
          <section className="flex flex-col gap-3">
            <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Provider keys
              <span className="ml-2 normal-case text-muted-foreground/60">
                At least one is required
              </span>
            </span>
            <div className="flex flex-col gap-2.5">
              {KEY_PROVIDERS.map((p) => (
                <KeyInput
                  key={p.id}
                  label={p.label}
                  sample={p.sample}
                  value={keys[p.id] ?? ""}
                  onChange={(v) => setKey(p.id, v)}
                />
              ))}
            </div>
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
          {!hasKey && (
            <p className="-mt-3 text-center text-[11px] text-muted-foreground/70">
              Add at least one provider key to continue.
            </p>
          )}
        </form>
      </div>
    </div>
  )
}

/** A single provider API-key input with a reveal toggle. */
function KeyInput({
  label,
  sample,
  value,
  onChange,
}: {
  label: string
  sample: string
  value: string
  onChange: (v: string) => void
}) {
  const [reveal, setReveal] = useState(false)
  return (
    <label className="flex items-center gap-3 rounded-md border border-border bg-background px-3 py-2">
      <span className="w-24 shrink-0 text-xs font-medium text-foreground/90">
        {label}
      </span>
      <input
        type={reveal ? "text" : "password"}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={sample}
        autoComplete="off"
        className="min-w-0 flex-1 rounded-md border border-border bg-card px-2 py-1 text-sm text-foreground
                   placeholder:text-muted-foreground/40
                   focus:border-signal focus:outline-none focus:ring-1 focus:ring-signal"
      />
      <button
        type="button"
        onClick={() => setReveal((r) => !r)}
        className="shrink-0 text-xs text-muted-foreground/60 transition-colors hover:text-foreground"
      >
        {reveal ? "Hide" : "Show"}
      </button>
    </label>
  )
}
