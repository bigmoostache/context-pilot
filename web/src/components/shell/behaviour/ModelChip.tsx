import { Check, Cpu } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { usePickerProviders, priceTag, type ModelDef, type ProviderDef } from "@/lib/support/models"
import { sendCommand } from "@/lib/live"
import { Tip } from "@/components/ui/tip"
import type { Agent } from "@/lib/types"

/** Text-only row highlight (mirrors {@link BehaviourChip}'s ROW_HILITE): brighten
 *  the ink on hover/focus, never wash the background. */
const ROW_HILITE =
  "transition-colors focus:bg-transparent! focus:text-foreground! focus:**:text-foreground! data-highlighted:bg-transparent! data-highlighted:text-foreground! data-highlighted:**:text-foreground!"

/** The OpenRouter provider id — the only provider that gets the extra
 *  `subprovider` grouping level (frontend-only, by the `vendor/` prefix). */
const OPENROUTER_ID = "openrouter"

/** Sub-provider label for an OpenRouter model: the `vendor` prefix of its
 *  `vendor/slug[:tag]` api name (e.g. `z-ai` from `z-ai/glm-5.3-flash`). This is
 *  the ONLY place the aggregator's sub-provider split lives — the backend keeps
 *  a flat model list; the differentiation is purely a footer-render concern. */
function subproviderOf(m: ModelDef): string {
  return m.apiName.split("/", 1)[0] ?? "other"
}

/** Group a provider's models by sub-provider, preserving first-seen order. */
function groupBySubprovider(models: ModelDef[]): [string, ModelDef[]][] {
  const groups = new Map<string, ModelDef[]>()
  for (const m of models) {
    const key = subproviderOf(m)
    const bucket = groups.get(key)
    if (bucket) bucket.push(m)
    else groups.set(key, [m])
  }
  return [...groups]
}

/** Resolve the active provider + model for the chip label + selected marks,
 *  from the agent's authoritative provider id + model apiName. */
function activeSelection(
  providers: ProviderDef[],
  agent: Agent | undefined,
): { provider: ProviderDef | undefined; model: ModelDef | undefined } {
  const provider =
    providers.find((p) => p.id === agent?.provider) ??
    providers.find((p) => p.models.some((m) => m.apiName === agent?.model))
  const model = provider?.models.find((m) => m.apiName === agent?.model) ?? provider?.models[0]
  return { provider, model }
}

/**
 * Active-model chip + picker — sits in the footer between the "Ready" status
 * indicator and the {@link BehaviourChip} system-prompt selector (T753). Shows
 * the loaded model's display name and, on click, a dropdown to switch the
 * provider and model.
 *
 * Two levels for most providers (provider rail → model rows); **three** for
 * OpenRouter, whose flat catalogue is grouped by sub-provider (the `vendor/`
 * prefix of each model's api name) — a purely frontend split, since the backend
 * treats OpenRouter as one flat provider (M mid-19: all real logic stays in the
 * Rust backend, the frontend only renders).
 *
 * Selecting a model issues the SAME `configure` command the settings picker
 * used (`sendCommand → POST /command → apply_command → apply_configure`); it
 * applies immediately, no save step. The active provider/model comes back
 * through the agent-meta fold, so the chip re-labels on the next delta.
 */
export function ModelChip({ agentId, agent }: { agentId: string; agent: Agent | undefined }) {
  const { data: providers = [] } = usePickerProviders()
  const { provider: activeProv, model: activeModel } = activeSelection(providers, agent)
  const label = activeModel?.displayName ?? "model"

  const select = (providerId: string, modelId: string) => {
    if (providerId === activeProv?.id && modelId === activeModel?.id) return
    void sendCommand(agentId, { kind: "configure", provider: providerId, model: modelId }).catch(
      () => {
        // Fire-and-forget: a failed switch keeps the current model; the agent-meta
        // fold re-reports ground truth on its next delta/poll.
      },
    )
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="flex cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground/85 focus:outline-none">
        <Cpu className="size-3.5" />
        <span className="max-w-[120px] truncate font-medium text-foreground/80">{label}</span>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        side="top"
        className="max-h-[70vh] min-w-72 overflow-y-auto"
      >
        {providers.length === 0 ? (
          <DropdownMenuItem disabled>No models available</DropdownMenuItem>
        ) : (
          providers.map((p, i) => (
            <ProviderGroup
              key={p.id}
              provider={p}
              activeProviderId={activeProv?.id}
              activeModelId={activeModel?.id}
              onSelect={select}
              showSeparator={i > 0}
            />
          ))
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** One provider's block: a labelled header, then its models — flat, or (for
 *  OpenRouter) split under sub-provider sub-headers. */
function ProviderGroup({
  provider,
  activeProviderId,
  activeModelId,
  onSelect,
  showSeparator,
}: {
  provider: ProviderDef
  activeProviderId: string | undefined
  activeModelId: string | undefined
  onSelect: (providerId: string, modelId: string) => void
  showSeparator: boolean
}) {
  const isOpenRouter = provider.id === OPENROUTER_ID
  return (
    <>
      {showSeparator && <DropdownMenuSeparator />}
      <DropdownMenuGroup>
        <DropdownMenuLabel className="flex items-center gap-1.5">
          <provider.icon className="size-3 text-muted-foreground/70" />
          {provider.name}
        </DropdownMenuLabel>
        {isOpenRouter
          ? groupBySubprovider(provider.models).map(([sub, models]) => (
              <SubproviderGroup
                key={sub}
                sub={sub}
                models={models}
                providerId={provider.id}
                active={provider.id === activeProviderId ? activeModelId : undefined}
                onSelect={onSelect}
              />
            ))
          : provider.models.map((m) => (
              <ModelRow
                key={m.id}
                model={m}
                active={provider.id === activeProviderId && m.id === activeModelId}
                onSelect={() => onSelect(provider.id, m.id)}
              />
            ))}
      </DropdownMenuGroup>
    </>
  )
}

/** OpenRouter sub-provider sub-header + its models. */
function SubproviderGroup({
  sub,
  models,
  providerId,
  active,
  onSelect,
}: {
  sub: string
  models: ModelDef[]
  providerId: string
  active: string | undefined
  onSelect: (providerId: string, modelId: string) => void
}) {
  return (
    <>
      <div className="px-2 pt-1.5 pb-0.5 text-[9.5px] font-semibold tracking-[0.07em] text-muted-foreground/60 uppercase">
        {sub}
      </div>
      {models.map((m) => (
        <ModelRow
          key={m.id}
          model={m}
          active={m.id === active}
          onSelect={() => onSelect(providerId, m.id)}
        />
      ))}
    </>
  )
}

/** One selectable model row: name + price tag, a check when active. */
function ModelRow({
  model,
  active,
  onSelect,
}: {
  model: ModelDef
  active: boolean
  onSelect: () => void
}) {
  return (
    <DropdownMenuItem
      onClick={onSelect}
      className={`justify-between gap-3 ${ROW_HILITE} ${
        active ? "font-semibold text-foreground" : "text-foreground/70"
      }`}
    >
      <span className="flex min-w-0 flex-1 items-center gap-2">
        {active ? (
          <Check className="size-3 shrink-0 text-(--interactive)" strokeWidth={3} />
        ) : (
          <span className="size-3 shrink-0" />
        )}
        <span className="truncate">{model.displayName}</span>
        {model.badge && (
          <span className="shrink-0 rounded-sm bg-muted/70 px-1 py-px text-[9px] font-semibold tracking-wide text-muted-foreground uppercase">
            {model.badge}
          </span>
        )}
      </span>
      <Tip title={model.apiName} side="right">
        <span className="shrink-0 font-mono text-[10px] text-muted-foreground/60 tabular-nums">
          {priceTag(model)}
        </span>
      </Tip>
    </DropdownMenuItem>
  )
}
