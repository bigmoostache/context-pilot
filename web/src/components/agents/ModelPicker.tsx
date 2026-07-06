import { Check } from "lucide-react"
import {
  findProvider,
  defaultModel,
  priceTag,
  type ProviderDef,
  type ModelDef,
} from "@/lib/support/models"
import { cn } from "@/lib/utils"

/**
 * Two-level LLM provider + model picker — the web counterpart of the TUI's
 * Ctrl+H cycle. Renders a horizontal provider rail (pills) above model cards
 * for the active provider. Shared by the per-agent manage modal and the global
 * defaults pane so both use the same registry and visual language.
 */
export function ModelPicker({
  providers,
  provider,
  model,
  onChange,
}: {
  /** The provider registry (fetched from backend). */
  providers: ProviderDef[]
  /** Active provider serde id (e.g. `"claudecodev2"`) */
  provider: string
  /** Active model serde id within the provider (e.g. `"claude-opus48"`) */
  model: string
  /** Fires on user pick — both values always set. */
  onChange: (provider: string, model: string) => void
}) {
  const activeProv = findProvider(providers, provider) ?? providers[0]

  // The registry loads async (useProviders) and an allowlist can filter it
  // down — guard the empty case so the picker never dereferences an undefined
  // provider (it simply renders nothing until models are available).
  if (!activeProv) {
    return (
      <div className="rounded-lg border border-border bg-muted/30 px-3 py-2.5 text-[12px] text-muted-foreground">
        No models available.
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      {/* provider rail */}
      <div className="flex flex-wrap gap-1.5">
        {providers.map((p) => {
          const on = p.id === activeProv.id
          return (
            <ProviderPill
              key={p.id}
              prov={p}
              active={on}
              onClick={() => {
                const dm = defaultModel(providers, p.id)
                onChange(p.id, dm?.id ?? p.models[0]?.id ?? "")
              }}
            />
          )
        })}
      </div>

      {/* model cards for the active provider */}
      <div className="flex flex-col gap-2">
        {activeProv.models.map((m, i) => {
          const on = m.id === model
          return (
            <ModelCard
              key={m.id}
              model={m}
              active={on}
              delay={i * 40}
              onClick={() => onChange(activeProv.id, m.id)}
            />
          )
        })}
      </div>
    </div>
  )
}

function ProviderPill({
  prov,
  active,
  onClick,
}: {
  prov: ProviderDef
  active: boolean
  onClick: () => void
}) {
  const Icon = prov.icon
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[11.5px] font-medium transition-all",
        active
          ? "bg-[var(--interactive)]/15 text-[var(--interactive)] ring-1 ring-[var(--interactive)]/30"
          : "bg-muted/50 text-muted-foreground hover:bg-muted hover:text-foreground/80",
      )}
    >
      <Icon className="size-3.5" />
      {prov.name}
    </button>
  )
}

function ModelCard({
  model: m,
  active,
  delay,
  onClick,
}: {
  model: ModelDef
  active: boolean
  delay: number
  onClick: () => void
}) {
  const Icon = m.icon
  return (
    <button
      onClick={onClick}
      style={{ animationDelay: `${delay}ms` }}
      className={cn(
        "opt-rise group flex items-center gap-3 rounded-xl border px-3 py-2.5 text-left transition-all",
        active
          ? "border-[var(--interactive)] bg-[var(--interactive)]/[0.07] ring-2 ring-[var(--interactive)]/15"
          : "border-border bg-card hover:border-[var(--interactive)]/40 hover:bg-muted/30",
      )}
    >
      <span
        className={cn(
          "flex size-8 shrink-0 items-center justify-center rounded-lg transition-colors",
          active
            ? "bg-[var(--interactive)]/15 text-[var(--interactive)]"
            : "bg-muted/60 text-muted-foreground/70",
        )}
      >
        <Icon className="size-4" />
      </span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-center gap-2">
          <span className="font-mono text-[12.5px] font-medium text-foreground/90">
            {m.displayName}
          </span>
          {m.badge && (
            <span className="rounded bg-muted/70 px-1.5 py-px text-[9.5px] font-semibold uppercase tracking-wide text-muted-foreground">
              {m.badge}
            </span>
          )}
        </span>
        <span className="text-[11px] text-muted-foreground">{m.apiName}</span>
      </div>
      <span className="shrink-0 font-mono text-[10.5px] tabular-nums text-muted-foreground/65">
        {priceTag(m)}
      </span>
      <span
        className={cn(
          "flex size-5 shrink-0 items-center justify-center rounded-full border transition-all",
          active
            ? "border-[var(--interactive)] bg-[var(--interactive)] text-[var(--primary-foreground)]"
            : "border-border text-transparent",
        )}
      >
        <Check className="size-3" strokeWidth={3} />
      </span>
    </button>
  )
}
