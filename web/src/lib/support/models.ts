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
import { getApiProviders } from "../api/generated/sdk.gen"
import { sdk } from "../api/client"

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
    case "Capable":
      return Sparkles
    case "Balanced":
      return Gauge
    case "Fast & cheap":
    case "Creative":
    case "Latest":
    case "Fastest":
    case "Cheap":
    case "Fast":
      return Zap
    default:
      return Bot
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

/** Singleton cache — providers never change during a session. */
let cached: ProviderDef[] | null = null

/** Fetch the provider registry (cached after first call). */
export async function fetchProviders(): Promise<ProviderDef[]> {
  if (cached) return cached
  // The client runs in responseStyle:"data" (setupClient), so the SDK call
  // resolves to the array directly — use sdk() like every other consumer
  // rather than destructuring a `{ data }` wrapper that doesn't exist at runtime.
  const data = await sdk<GenProviderDef[]>(getApiProviders({ throwOnError: true }))
  cached = enrichProviders(data)
  return cached
}

/**
 * Synchronous access to the cached providers. Returns `null` if not yet
 * fetched. Components should call `fetchProviders()` first (e.g. via
 * TanStack Query) and use this only as a fast path.
 */
export function getCachedProviders(): ProviderDef[] | null {
  return cached
}

/** TanStack Query hook — fetches once, caches forever (providers never change). */
export function useProviders() {
  return useQuery({
    queryKey: ["providers"],
    queryFn: fetchProviders,
    staleTime: Infinity,
  })
}

// ── Compact price formatter ───────────────────────────────────────────

/** `$5 · 200K` style label for the picker card. */
export function priceTag(m: GenModelDef): string {
  const ctx =
    m.contextWindow >= 1_000_000
      ? `${(m.contextWindow / 1_000_000).toFixed(0)}M`
      : `${(m.contextWindow / 1_000).toFixed(0)}K`
  return `$${m.inputPrice} · ${ctx}`
}

// ── Allowlist ──────────────────────────────────────────────────────────

/** Compound id used by the org allowlist setting: `"<providerId>:<modelId>"`. */
export function modelKey(providerId: string, modelId: string): string {
  return `${providerId}:${modelId}`
}

/**
 * Restrict providers' models to the org allowlist of `"provider:model"` ids.
 * An **empty** allowlist means everything is allowed (delivery default), so the
 * registry passes through untouched. Providers left with no allowed model are
 * dropped so the picker never shows an empty provider.
 */
export function filterAllowed(
  providers: ProviderDef[],
  allowed: string[],
): ProviderDef[] {
  if (allowed.length === 0) return providers
  const set = new Set(allowed)
  return providers
    .map((p) => ({
      ...p,
      models: p.models.filter((m) => set.has(modelKey(p.id, m.id))),
    }))
    .filter((p) => p.models.length > 0)
}

// ── Lookup helpers ─────────────────────────────────────────────────────

/** Find a provider by its serde id. */
export function findProvider(
  providers: ProviderDef[],
  id: string,
): ProviderDef | undefined {
  return providers.find((p) => p.id === id)
}

/** Find a model within a provider by its serde id. */
export function findModel(
  providers: ProviderDef[],
  providerId: string,
  modelId: string,
): ModelDef | undefined {
  return findProvider(providers, providerId)?.models.find(
    (m) => m.id === modelId,
  )
}

/** Get the default model for a provider. */
export function defaultModel(
  providers: ProviderDef[],
  providerId: string,
): ModelDef | undefined {
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
  const provider = providerId
    ? findProvider(providers, providerId)
    : undefined
  if (provider) {
    const model =
      (apiName && provider.models.find((m) => m.apiName === apiName)) ||
      defaultModel(providers, provider.id)
    if (model) return { provider, model }
  }
  return apiName ? resolveFromApiName(providers, apiName) : undefined
}
