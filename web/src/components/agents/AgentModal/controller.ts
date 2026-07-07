import { useState } from "react"
import type { Agent } from "@/lib/types"
import { defaultModel, resolveSelection, type ProviderDef } from "@/lib/support/models"

/** The two ways the dialog can open. */
export type AgentModalMode = { mode: "create" } | { mode: "manage"; agent: Agent }

/** A resolved provider+model id pair for the picker. */
export interface Selection {
  p: string
  m: string
}

/** The persisted global picker defaults, read from localStorage. */
interface LsDefaults {
  provider: string | null
  model: string | null
}

/** Derive the realm folder name from the agent name (replaces the folder picker). */
export function slugify(name: string): string {
  const s = name
    .trim()
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, "-")
    .replaceAll(/^-+|-+$/g, "")
  return s || "untitled"
}

/** The default model id for a provider, or the first available, or "" — the
 *  shared `??`-chain lifted out so the selection resolvers stay within the P8
 *  complexity budget (each `?.`/`??` counts as a branch). */
function firstModelId(providers: ProviderDef[], providerId: string): string {
  return defaultModel(providers, providerId)?.id ?? providers[0]?.models[0]?.id ?? ""
}

/** Manage-mode seed: the agent's authoritative provider/model, falling back to
 *  the first provider + its default model on a cold (empty) registry. */
function manageSelection(agent: Agent, providers: ProviderDef[]): Selection {
  const resolved = resolveSelection(providers, agent.provider, agent.model)
  const p = resolved?.provider.id ?? providers[0]?.id ?? ""
  return { p, m: resolved?.model.id ?? firstModelId(providers, p) }
}

/** Create-mode seed: the persisted localStorage defaults, falling back to the
 *  first provider + its default model. */
function createSelection(providers: ProviderDef[], ls: LsDefaults): Selection {
  const p = ls.provider ?? providers[0]?.id ?? ""
  return { p, m: ls.model ?? firstModelId(providers, p) }
}

/**
 * Resolve the picker's initial selection at mount. On a cold render the provider
 * registry is empty, so this falls back to the persisted localStorage defaults
 * (create) or the agent's authoritative provider/model (manage); the sync pass
 * below corrects it once providers load.
 */
function initialSelection(
  isManage: boolean,
  agent: Agent | undefined,
  providers: ProviderDef[],
  ls: LsDefaults,
): Selection {
  return isManage && agent ? manageSelection(agent, providers) : createSelection(providers, ls)
}

/**
 * The corrected selection once the provider registry arrives, or null when the
 * current selection already stands. Mirrors {@link initialSelection} but is used
 * by the one-shot render-phase back-fill.
 */
function syncedSelection(
  isManage: boolean,
  agent: Agent | undefined,
  providers: ProviderDef[],
  ls: LsDefaults,
): Selection | null {
  if (isManage && agent) {
    const sel = resolveSelection(providers, agent.provider, agent.model)
    return sel ? { p: sel.provider.id, m: sel.model.id } : null
  }
  const p = ls.provider ?? providers[0]?.id ?? ""
  if (!p) return null
  return { p, m: ls.model ?? firstModelId(providers, p) }
}

/** The picker's selection state + the cold-load back-fill. */
export interface SelectionState {
  provId: string
  modelId: string
  setSel: (p: string, m: string) => void
}

/**
 * Own the provider/model selection: seed it from localStorage / the agent at
 * mount, then back-fill once the provider registry loads (cold refresh starts
 * empty). The back-fill is a render-phase compare against a `synced` sentinel —
 * NOT an effect (which would trip set-state-in-effect and cost an extra commit);
 * the guard flips permanently after the first providers arrival so a manual
 * change is never clobbered.
 */
export function useSelectionState(
  isManage: boolean,
  agent: Agent | undefined,
  providers: ProviderDef[],
): SelectionState {
  // localStorage picker defaults, read ONCE at mount (a lazy initializer — the
  // only place @eslint-react/purity permits a side-effecting read).
  const [ls] = useState<LsDefaults>(() => ({
    provider: localStorage.getItem("cp-default-provider"),
    model: localStorage.getItem("cp-default-model"),
  }))
  const [seed] = useState(() => initialSelection(isManage, agent, providers, ls))
  const [provId, setProvId] = useState(seed.p)
  const [modelId, setModelId] = useState(seed.m)
  const setSel = (p: string, m: string) => {
    setProvId(p)
    setModelId(m)
  }

  const [synced, setSynced] = useState(false)
  if (!synced && providers.length > 0) {
    const sel = syncedSelection(isManage, agent, providers, ls)
    if (sel) setSel(sel.p, sel.m)
    setSynced(true)
  }

  return { provId, modelId, setSel }
}
