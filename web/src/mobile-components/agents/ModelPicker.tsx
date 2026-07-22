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
 * ModelPicker`, rebuilt to feel iOS-native rather than a desktop transcription.
 *
 * The old mobile version put providers in a **horizontal-scroll pill rail**
 * (`overflow-x-auto no-scrollbar`) — providers off the right edge were invisible
 * (no scrollbar hint) and the sideways scroll fought the page's vertical scroll.
 * Now the providers **wrap** (`flex-wrap`) so every one is visible at once, and
 * the models render as a **grouped inset list** (the iOS Settings idiom): a
 * rounded card of full-width tap rows with hairline dividers and a trailing
 * checkmark on the selected model — instead of the heavy bordered cards.
 *
 * Same registry, same two-level shape, same empty-guard. Shared by the mobile
 * AgentModal (Agent Settings page) and the mobile config GeneralPane.
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
      <div className="rounded-2xl border border-border/60 bg-card px-4 py-3 text-[13px] text-muted-foreground">
        No models available.
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-2.5">
      {/* Provider chips — WRAP (no horizontal scroll) so every provider is
          visible at once, no hidden overflow. */}
      <div className="flex flex-wrap gap-1.5">
        {providers.map((p) => {
          const on = p.id === activeProv.id
          return (
            <ProviderChip
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

      {/* Model list — an iOS grouped inset list: tap a row to select, checkmark
          marks the active one. */}
      <div className="divide-y divide-border/50 overflow-hidden rounded-2xl border border-border/60 bg-card">
        {activeProv.models.map((m) => (
          <ModelRow
            key={m.id}
            model={m}
            active={m.id === model}
            onClick={() => onChange(activeProv.id, m.id)}
          />
        ))}
      </div>
    </div>
  )
}

/** A provider selector chip — wraps in the provider row rather than scrolling. */
function ProviderChip({
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
        "flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12.5px] font-medium transition-all",
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

/**
 * One model as an iOS list row: leading provider glyph, model name + api-name
 * subtitle, trailing price, and a checkmark when it's the selected model. The
 * whole row is the tap target; the selected row carries a faint tint.
 */
function ModelRow({
  model: m,
  active,
  onClick,
}: {
  model: ModelDef
  active: boolean
  onClick: () => void
}) {
  const Icon = m.icon
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-3 px-4 py-3 text-left transition-colors",
        active ? "bg-(--interactive)/6" : "active:bg-muted/40",
      )}
    >
      <span
        className={cn(
          "flex size-8 shrink-0 items-center justify-center rounded-lg transition-colors",
          active
            ? "bg-(--interactive)/15 text-(--interactive)"
            : "bg-muted/60 text-muted-foreground/70",
        )}
      >
        <Icon className="size-4" />
      </span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-center gap-2">
          <span className="font-mono text-[14px] font-medium text-foreground/90">
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
      {/* Checkmark marks the selected model (the iOS radio-row convention). */}
      <Check
        className={cn(
          "size-4 shrink-0 text-(--interactive) transition-opacity",
          active ? "opacity-100" : "opacity-0",
        )}
        strokeWidth={3}
      />
    </button>
  )
}
