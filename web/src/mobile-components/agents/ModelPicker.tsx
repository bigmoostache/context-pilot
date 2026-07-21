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
 * Two-level provider + model picker — mobile twin of `components/agents/
 * ModelPicker`.
 *
 * Same registry, same two-level shape (provider rail above model cards), same
 * empty-guard. The fork is touch sizing: the provider pills and model cards grow
 * their tap padding (`py-2` / `py-3`), the provider rail scrolls horizontally
 * (`no-scrollbar overflow-x-auto`) instead of wrapping, and press feedback swaps
 * `hover:` for `active:`. Shared by the mobile AgentModal and the mobile config
 * GeneralPane.
 */
export function ModelPicker({
  providers,
  provider,
  model,
  onChange,
}: {
  providers: ProviderDef[]
  provider: string
  model: string
  onChange: (provider: string, model: string) => void
}) {
  const activeProv = findProvider(providers, provider) ?? providers[0]

  // The registry loads async and an allowlist can filter it down — guard the
  // empty case so the picker never dereferences an undefined provider.
  if (!activeProv) {
    return (
      <div className="rounded-lg border border-border bg-muted/30 px-3 py-2.5 text-[12.5px] text-muted-foreground">
        No models available.
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      {/* provider rail — horizontally scrollable on mobile */}
      <div className="no-scrollbar flex gap-1.5 overflow-x-auto">
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
        "flex shrink-0 items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium transition-all",
        active
          ? "bg-(--interactive)/15 text-(--interactive) ring-1 ring-(--interactive)/30"
          : "bg-muted/50 text-muted-foreground active:bg-muted active:text-foreground/80",
      )}
    >
      <Icon className="size-4" />
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
        "opt-rise group flex items-center gap-3 rounded-xl border p-3 text-left transition-all",
        active
          ? "border-(--interactive) bg-(--interactive)/[0.07] ring-2 ring-(--interactive)/15"
          : "border-border bg-card active:border-(--interactive)/40 active:bg-muted/30",
      )}
    >
      <span
        className={cn(
          "flex size-9 shrink-0 items-center justify-center rounded-lg transition-colors",
          active
            ? "bg-(--interactive)/15 text-(--interactive)"
            : "bg-muted/60 text-muted-foreground/70",
        )}
      >
        <Icon className="size-4" />
      </span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-center gap-2">
          <span className="font-mono text-[13px] font-medium text-foreground/90">
            {m.displayName}
          </span>
          {m.badge && (
            <span className="rounded-sm bg-muted/70 px-1.5 py-px text-[9.5px] font-semibold tracking-wide text-muted-foreground uppercase">
              {m.badge}
            </span>
          )}
        </span>
        <span className="truncate text-[11.5px] text-muted-foreground">{m.apiName}</span>
      </div>
      <span className="shrink-0 font-mono text-[10.5px] text-muted-foreground/65 tabular-nums">
        {priceTag(m)}
      </span>
      <span
        className={cn(
          "flex size-5 shrink-0 items-center justify-center rounded-full border transition-all",
          active
            ? "border-(--interactive) bg-(--interactive) text-(--primary-foreground)"
            : "border-border text-transparent",
        )}
      >
        <Check className="size-3" strokeWidth={3} />
      </span>
    </button>
  )
}
