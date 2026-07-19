// ── LLM provider + model registry ─────────────────────────────────────
//
// Types and data fetched from the backend `GET /api/providers` endpoint.
// The backend (providers.rs) is the single source of truth for pricing,
// context windows, and model catalogs. This file only adds frontend-only
// decoration (Lucide icons) and provides lookup helpers.

import type { LucideIcon } from "lucide-react"
import { Sparkles, Gauge, Zap, Bot, Cpu, KeyRound, ShieldCheck } from "lucide-react"
import { useQuery } from "@tanstack/react-query"
import type {
  ModelDef as GenModelDef,
  ProviderDef as GenProviderDef,
} from "../api/generated/types.gen"
import { fetchProviderDefs } from "../api"

// ── Types (generated base + frontend icon) ────────────────────────────

export interface ModelDef extends GenModelDef {
  icon: LucideIcon
}

export interface ProviderDef extends Omit<GenProviderDef, "models"> {
  icon: LucideIcon
  models: ModelDef[]
}

// ── Icon mapping (the ONLY frontend-maintained data) ──────────────────

const PROVIDER_ICONS: Record<string, LucideIcon> = {
  claudecodev2: Cpu,
  anthropic: Sparkles,
  claudecode: ShieldCheck,
  claudecodeapikey: KeyRound,
  grok: Zap,
  groq: Gauge,
  deepseek: Bot,
  minimax: Bot,
}

/** Map a model badge string to an icon. Falls back to Bot for unknowns. */
function modelIcon(badge: string | undefined | null): LucideIcon {
  switch (badge) {
    case "Most capable":
    case "Large":
    case "Capable": {
      return Sparkles
    }
    case "Balanced": {
      return Gauge
    }
    case "Fast & cheap":
    case "Creative":
    case "Latest":
    case "Fastest":
    case "Cheap":
    case "Fast": {
      return Zap
    }
    default: {
      return Bot
    }
  }
}

// ── Enrichment (raw API → frontend-ready) ─────────────────────────────

function enrichProviders(raw: GenProviderDef[]): ProviderDef[] {
  return raw.map((p) => ({
    ...p,
    icon: PROVIDER_ICONS[p.id] ?? Bot,
    models: p.models.map((m) => ({
      ...m,
      icon: modelIcon(m.badge),
    })),
  }))
}

// ── Data fetching ─────────────────────────────────────────────────────

/** Singleton cache — providers never change during a session. Held in an
 *  object so the memoising write is a property mutation, not a reassignment of a
 *  module-level binding from inside a function. */
const providerCache: { value: ProviderDef[] | null } = { value: null }

/** Drop the memoised registry so the next `fetchProviders` re-asks the server.
 *  Call after any event that can change provider *usability* — a Claude OAuth
 *  login, account switch, or token refresh — then invalidate the `["providers"]`
 *  query so mounted pickers refetch instead of reading a stale singleton. */
export function resetProviderCache(): void {
  providerCache.value = null
}

/** Fetch the full usable provider registry (cached after first call). The
 *  backend already drops providers without a configured key and stamps each
 *  model with its canonical `key`, so this is the admin-facing catalog (every
 *  usable model, unfiltered by the org allowlist). */
export async function fetchProviders(): Promise<ProviderDef[]> {
  // An empty registry is a valid but *transient* state — it means nothing was
  // usable when we last asked (e.g. before the Claude OAuth login). Memoising it
  // would be fatal: `[]` is truthy, so it would pin "No models available"
  // forever, even after login makes providers appear. Only cache a non-empty
  // result; otherwise fall through and re-ask the server on the next call.
  if (providerCache.value?.length) return providerCache.value
  const data = await fetchProviderDefs()
  providerCache.value = enrichProviders(data)
  return providerCache.value
}

/** Fetch the picker registry — usable providers with the org model allowlist
 *  already applied server-side (`?allowed=1`). This is what end users pick from;
 *  it depends on the allowlist, so it's never part of the static singleton and
 *  is invalidated (query key `["providers", "picker"]`) when the admin edits the
 *  allowlist. */
export async function fetchPickerProviders(): Promise<ProviderDef[]> {
  const data = await fetchProviderDefs(true)
  return enrichProviders(data)
}

/** TanStack Query hook — fetches once, caches forever (providers never change). */
export function useProviders() {
  return useQuery({
    queryKey: ["providers"],
    queryFn: fetchProviders,
    staleTime: Infinity,
  })
}

/** TanStack Query hook for the allowlist-filtered picker registry. Cached until
 *  the allowlist changes — `ConfigPanes` invalidates `["providers", "picker"]`
 *  after a save. */
export function usePickerProviders() {
  return useQuery({
    queryKey: ["providers", "picker"],
    queryFn: fetchPickerProviders,
    staleTime: Infinity,
  })
}

// ── Compact price formatter ───────────────────────────────────────────

/** `$5 · 200K` style label for the picker card. */
export function priceTag(m: GenModelDef): string {
  const ctx =
    m.contextWindow >= 1_000_000
      ? `${(m.contextWindow / 1_000_000).toFixed(0)}M`
      : `${(m.contextWindow / 1000).toFixed(0)}K`
  return `$${m.inputPrice} · ${ctx}`
}

// ── Lookup helpers ─────────────────────────────────────────────────────

/** Find a provider by its serde id. */
export function findProvider(providers: ProviderDef[], id: string): ProviderDef | undefined {
  return providers.find((p) => p.id === id)
}

/** Find a model within a provider by its serde id. */
export function findModel(
  providers: ProviderDef[],
  providerId: string,
  modelId: string,
): ModelDef | undefined {
  return findProvider(providers, providerId)?.models.find((m) => m.id === modelId)
}

/** Get the default model for a provider. */
export function defaultModel(providers: ProviderDef[], providerId: string): ModelDef | undefined {
  const p = findProvider(providers, providerId)
  return p?.models.find((m) => m.isDefault) ?? p?.models[0]
}

/**
 * Resolve a provider+model pair from an API model name string.
 * Searches all providers for a model whose `apiName` matches.
 */
export function resolveFromApiName(
  providers: ProviderDef[],
  apiName: string,
): { provider: ProviderDef; model: ModelDef } | undefined {
  for (const p of providers) {
    const m = p.models.find((m) => m.apiName === apiName)
    if (m) return { provider: p, model: m }
  }
  return undefined
}

/**
 * Resolve the picker's provider+model selection for an agent, preferring the
 * agent's authoritative provider id over guessing from the model name.
 */
export function resolveSelection(
  providers: ProviderDef[],
  providerId: string | undefined,
  apiName: string | undefined,
): { provider: ProviderDef; model: ModelDef } | undefined {
  const provider = providerId ? findProvider(providers, providerId) : undefined
  if (provider) {
    const model =
      (apiName && provider.models.find((m) => m.apiName === apiName)) ||
      defaultModel(providers, provider.id)
    if (model) return { provider, model }
  }
  return apiName ? resolveFromApiName(providers, apiName) : undefined
}
