// ── LLM provider + model registry ─────────────────────────────────────
//
// The web counterpart of the TUI's Ctrl+H provider/model cycle. Mirrors
// every provider and model enum from cp-base/src/config/{llm_types,models}.rs
// with their pricing, context windows, and display metadata. The registry is
// the single source of truth for both the per-agent manage modal and the
// global defaults pane — no hardcoded model lists elsewhere.
//
// IDs use the EXACT serde names so a Configure command carries the same
// string the agent deserializes directly.

import type { LucideIcon } from "lucide-react"
import { Sparkles, Gauge, Zap, Bot, Cpu, KeyRound, ShieldCheck } from "lucide-react"

// ── Types ─────────────────────────────────────────────────────────────

export interface ModelDef {
  /** serde kebab-case id, e.g. `"claude-opus-45"` */
  id: string
  /** API wire name, e.g. `"claude-opus-4-6"` */
  apiName: string
  /** Human label, e.g. `"Opus 4.6"` */
  displayName: string
  /** Max context window tokens */
  contextWindow: number
  /** Max output tokens per response */
  maxOutput: number
  /** Input price per million tokens (USD) */
  inputPrice: number
  /** Output price per million tokens (USD) */
  outputPrice: number
  /** Short tier badge, e.g. `"Most capable"` */
  badge?: string
  /** Icon for the picker card */
  icon: LucideIcon
  /** True if this is the provider's default model */
  isDefault?: boolean
}

export interface ProviderDef {
  /** serde lowercase id, e.g. `"anthropic"` */
  id: string
  /** Human label, e.g. `"Anthropic"` */
  name: string
  /** One-line description */
  description: string
  /** Icon for the provider tab */
  icon: LucideIcon
  /** Ordered list of models for this provider */
  models: ModelDef[]
}

// ── Compact price formatter ───────────────────────────────────────────

/** `$5 · 200K` style label for the picker card. */
export function priceTag(m: ModelDef): string {
  const ctx =
    m.contextWindow >= 1_000_000
      ? `${(m.contextWindow / 1_000_000).toFixed(0)}M`
      : `${(m.contextWindow / 1_000).toFixed(0)}K`
  return `$${m.inputPrice} · ${ctx}`
}

// ── Registry ──────────────────────────────────────────────────────────

export const PROVIDERS: ProviderDef[] = [
  {
    id: "claudecodev2",
    name: "Claude Code V2",
    description: "OAuth — Opus 4.8 · Fable 5 · Sonnet 4.6",
    icon: Cpu,
    models: [
      {
        id: "claude-opus48",
        apiName: "claude-opus-4-8",
        displayName: "Opus 4.8",
        contextWindow: 200_000,
        maxOutput: 64_000,
        inputPrice: 5,
        outputPrice: 25,
        badge: "Most capable",
        icon: Sparkles,
        isDefault: true,
      },
      {
        id: "claude-sonnet46",
        apiName: "claude-sonnet-4-6",
        displayName: "Sonnet 4.6",
        contextWindow: 1_000_000,
        maxOutput: 64_000,
        inputPrice: 3,
        outputPrice: 15,
        badge: "Balanced",
        icon: Gauge,
      },
      {
        id: "claude-fable5",
        apiName: "claude-fable-5",
        displayName: "Fable 5",
        contextWindow: 400_000,
        maxOutput: 64_000,
        inputPrice: 10,
        outputPrice: 50,
        badge: "Creative",
        icon: Zap,
      },
    ],
  },
  {
    id: "anthropic",
    name: "Anthropic",
    description: "Direct API — Opus 4.5 · Sonnet 4.5 · Haiku 4.5",
    icon: Sparkles,
    models: [
      {
        id: "claude-opus45",
        apiName: "claude-opus-4-6",
        displayName: "Opus 4.5",
        contextWindow: 200_000,
        maxOutput: 128_000,
        inputPrice: 5,
        outputPrice: 25,
        badge: "Most capable",
        icon: Sparkles,
        isDefault: true,
      },
      {
        id: "claude-sonnet45",
        apiName: "claude-sonnet-4-5-20250929",
        displayName: "Sonnet 4.5",
        contextWindow: 200_000,
        maxOutput: 64_000,
        inputPrice: 3,
        outputPrice: 15,
        badge: "Balanced",
        icon: Gauge,
      },
      {
        id: "claude-haiku45",
        apiName: "claude-haiku-4-5-20251001",
        displayName: "Haiku 4.5",
        contextWindow: 200_000,
        maxOutput: 64_000,
        inputPrice: 1,
        outputPrice: 5,
        badge: "Fast & cheap",
        icon: Zap,
      },
    ],
  },
  {
    id: "claudecode",
    name: "Claude Code (OAuth)",
    description: "OAuth V1 — Opus 4.5 · Sonnet 4.5 · Haiku 4.5",
    icon: ShieldCheck,
    models: [
      {
        id: "claude-opus45",
        apiName: "claude-opus-4-6",
        displayName: "Opus 4.5",
        contextWindow: 200_000,
        maxOutput: 128_000,
        inputPrice: 5,
        outputPrice: 25,
        badge: "Most capable",
        icon: Sparkles,
        isDefault: true,
      },
      {
        id: "claude-sonnet45",
        apiName: "claude-sonnet-4-5-20250929",
        displayName: "Sonnet 4.5",
        contextWindow: 200_000,
        maxOutput: 64_000,
        inputPrice: 3,
        outputPrice: 15,
        badge: "Balanced",
        icon: Gauge,
      },
      {
        id: "claude-haiku45",
        apiName: "claude-haiku-4-5-20251001",
        displayName: "Haiku 4.5",
        contextWindow: 200_000,
        maxOutput: 64_000,
        inputPrice: 1,
        outputPrice: 5,
        badge: "Fast & cheap",
        icon: Zap,
      },
    ],
  },
  {
    id: "claudecodeapikey",
    name: "Claude Code (API Key)",
    description: "API key — Opus 4.5 · Sonnet 4.5 · Haiku 4.5",
    icon: KeyRound,
    models: [
      {
        id: "claude-opus45",
        apiName: "claude-opus-4-6",
        displayName: "Opus 4.5",
        contextWindow: 200_000,
        maxOutput: 128_000,
        inputPrice: 5,
        outputPrice: 25,
        badge: "Most capable",
        icon: Sparkles,
        isDefault: true,
      },
      {
        id: "claude-sonnet45",
        apiName: "claude-sonnet-4-5-20250929",
        displayName: "Sonnet 4.5",
        contextWindow: 200_000,
        maxOutput: 64_000,
        inputPrice: 3,
        outputPrice: 15,
        badge: "Balanced",
        icon: Gauge,
      },
      {
        id: "claude-haiku45",
        apiName: "claude-haiku-4-5-20251001",
        displayName: "Haiku 4.5",
        contextWindow: 200_000,
        maxOutput: 64_000,
        inputPrice: 1,
        outputPrice: 5,
        badge: "Fast & cheap",
        icon: Zap,
      },
    ],
  },
  {
    id: "grok",
    name: "xAI Grok",
    description: "Fast tool-calling · 2M context",
    icon: Zap,
    models: [
      {
        id: "grok41-fast",
        apiName: "grok-4-1-fast",
        displayName: "Grok 4.1 Fast",
        contextWindow: 2_000_000,
        maxOutput: 128_000,
        inputPrice: 0.2,
        outputPrice: 0.5,
        badge: "Latest",
        icon: Zap,
        isDefault: true,
      },
      {
        id: "grok4-fast",
        apiName: "grok-4-fast",
        displayName: "Grok 4 Fast",
        contextWindow: 2_000_000,
        maxOutput: 128_000,
        inputPrice: 0.2,
        outputPrice: 0.5,
        icon: Zap,
      },
    ],
  },
  {
    id: "groq",
    name: "Groq",
    description: "Ultra-fast inference · GPT-OSS · Llama",
    icon: Gauge,
    models: [
      {
        id: "gpt-oss120b",
        apiName: "openai/gpt-oss-120b",
        displayName: "GPT-OSS 120B (+web)",
        contextWindow: 131_072,
        maxOutput: 128_000,
        inputPrice: 1.2,
        outputPrice: 1.2,
        badge: "Large",
        icon: Sparkles,
        isDefault: true,
      },
      {
        id: "gpt-oss20b",
        apiName: "openai/gpt-oss-20b",
        displayName: "GPT-OSS 20B (+web)",
        contextWindow: 131_072,
        maxOutput: 128_000,
        inputPrice: 0.2,
        outputPrice: 0.2,
        icon: Gauge,
      },
      {
        id: "llama33-70b",
        apiName: "llama-3.3-70b-versatile",
        displayName: "Llama 3.3 70B",
        contextWindow: 131_072,
        maxOutput: 128_000,
        inputPrice: 0.59,
        outputPrice: 0.79,
        icon: Bot,
      },
      {
        id: "llama31-8b",
        apiName: "llama-3.1-8b-instant",
        displayName: "Llama 3.1 8B",
        contextWindow: 131_072,
        maxOutput: 128_000,
        inputPrice: 0.05,
        outputPrice: 0.08,
        badge: "Fastest",
        icon: Zap,
      },
    ],
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    description: "V4 Flash & Pro · 1M context",
    icon: Bot,
    models: [
      {
        id: "v4-flash",
        apiName: "deepseek-v4-flash",
        displayName: "V4 Flash",
        contextWindow: 1_000_000,
        maxOutput: 384_000,
        inputPrice: 0.14,
        outputPrice: 0.28,
        badge: "Cheap",
        icon: Zap,
        isDefault: true,
      },
      {
        id: "v4-pro",
        apiName: "deepseek-v4-pro",
        displayName: "V4 Pro",
        contextWindow: 1_000_000,
        maxOutput: 384_000,
        inputPrice: 0.435,
        outputPrice: 0.87,
        badge: "Capable",
        icon: Sparkles,
      },
    ],
  },
  {
    id: "minimax",
    name: "MiniMax",
    description: "M2.7 — Anthropic-compatible API",
    icon: Bot,
    models: [
      {
        id: "m27",
        apiName: "MiniMax-M2.7",
        displayName: "M2.7",
        contextWindow: 204_800,
        maxOutput: 128_000,
        inputPrice: 2,
        outputPrice: 8,
        icon: Bot,
        isDefault: true,
      },
      {
        id: "m27-highspeed",
        apiName: "MiniMax-M2.7",
        displayName: "M2.7 Highspeed",
        contextWindow: 131_072,
        maxOutput: 128_000,
        inputPrice: 4,
        outputPrice: 16,
        badge: "Fast",
        icon: Zap,
      },
    ],
  },
]

// ── Lookup helpers ─────────────────────────────────────────────────────

/** Find a provider by its serde id. */
export function findProvider(id: string): ProviderDef | undefined {
  return PROVIDERS.find((p) => p.id === id)
}

/** Find a model within a provider by its serde id. */
export function findModel(
  providerId: string,
  modelId: string,
): ModelDef | undefined {
  return findProvider(providerId)?.models.find((m) => m.id === modelId)
}

/** Get the default model for a provider. */
export function defaultModel(providerId: string): ModelDef | undefined {
  const p = findProvider(providerId)
  return p?.models.find((m) => m.isDefault) ?? p?.models[0]
}

/**
 * Resolve a provider+model pair from an API model name string.
 * Searches all providers for a model whose `apiName` matches.
 * Returns `undefined` if no match (unknown model).
 */
export function resolveFromApiName(
  apiName: string,
): { provider: ProviderDef; model: ModelDef } | undefined {
  for (const p of PROVIDERS) {
    const m = p.models.find((m) => m.apiName === apiName)
    if (m) return { provider: p, model: m }
  }
  return undefined
}

/**
 * Resolve the picker's provider+model selection for an agent, preferring the
 * agent's **authoritative** provider id over guessing from the model name.
 *
 * Several providers share identical model API names — every Claude-Code variant
 * (`claudecode`, `claudecodeapikey`, `claudecodev2`) reuses the Anthropic model
 * roster — so resolving from the API name alone always collapses onto the first
 * matching provider (Anthropic), which is the "shows Anthropic for a Claude
 * Code V2 agent" bug. When the backend supplies the real provider id we honor
 * it and only use the model name to pick the row within that provider (falling
 * back to its default model). Without a provider id we degrade to the old
 * name-only resolution.
 */
export function resolveSelection(
  providerId: string | undefined,
  apiName: string | undefined,
): { provider: ProviderDef; model: ModelDef } | undefined {
  const provider = providerId ? findProvider(providerId) : undefined
  if (provider) {
    const model =
      (apiName && provider.models.find((m) => m.apiName === apiName)) ||
      defaultModel(provider.id)
    if (model) return { provider, model }
  }
  return apiName ? resolveFromApiName(apiName) : undefined
}
