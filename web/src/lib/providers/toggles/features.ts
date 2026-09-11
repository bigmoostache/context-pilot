import { createContext, use } from "react"
import type { Features } from "@/lib/api"

/**
 * Server-driven **feature flags** (`GET /api/features`, public, pre-login).
 *
 * Unlike the sibling toggles (dev mode, overlay, aside), these are NOT a client
 * preference: the deployment decides them through `CP_FEATURE_*` variables
 * (see `docs/ENV.md`) and the backend enforces them (a switched-off surface
 * answers 404). The cockpit only mirrors them — hiding the day-0 setup where
 * there is none, the Claude subscription where it is not offered, the IT and
 * Update panes where the box has nothing to manage, and the key editor where
 * keys come from the environment alone.
 *
 * The provider component lives in `./FeaturesProvider` (split out so this
 * module exports no component, satisfying the Fast-Refresh purity rule).
 */
export interface FeaturesCtx {
  /** The effective flags (defaults until the backend answered). */
  features: Features
  /** True once the backend answered (or failed — defaults then apply). */
  loaded: boolean
}

/** What the backend reports when unset — mirrors the `cp-env` table defaults. */
export const FEATURE_DEFAULTS: Features = {
  claude_oauth: true,
  day0_setup: false,
  it_pane: false,
  updater: false,
  keys_editable: true,
  onboarding: true,
}

/** Feature-flags context object. Supplied by `FeaturesProvider`, read by {@link useFeatures}. */
export const FeaturesContext = createContext<FeaturesCtx | null>(null)

/** The deployment's feature flags. Must be called inside a FeaturesProvider. */
export function useFeatures(): Features {
  return useFeaturesCtx().features
}

/** Whether the flags have been fetched (the guard waits for this so gated UI never flashes). */
export function useFeaturesLoaded(): boolean {
  return useFeaturesCtx().loaded
}

function useFeaturesCtx(): FeaturesCtx {
  const ctx = use(FeaturesContext)
  if (!ctx) throw new Error("useFeatures must be used within FeaturesProvider")
  return ctx
}
