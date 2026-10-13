import { createContext, use } from "react"

/**
 * Global **default visibility** preference for the agent right aside (T677).
 *
 * The Files/Tasks rail beside a conversation can be shown or hidden per-agent
 * (that per-agent choice is persisted by {@link useAgentAside} under its own
 * key). THIS flag is the org-wide *default* a agent with no stored choice
 * falls back to — configured once in Settings › General.
 *
 * Pure client-side view preference, persisted to `localStorage` only, mirroring
 * the show-overlay toggle. Stored under `cp-aside-default-hidden`; the value is
 * the literal `"1"` when the default is **hidden**, and anything else resolves
 * to **shown** — guaranteeing the requested "default: show".
 *
 * The provider component lives in `./AsideDefaultProvider` (split out so this
 * module exports no component, satisfying the Fast-Refresh purity rule).
 */
export interface AsideDefaultCtx {
  /** When true, a agent with no stored per-agent choice starts HIDDEN. */
  defaultHidden: boolean
  setDefaultHidden: (hidden: boolean) => void
  toggle: () => void
}

/** Aside-default context object. Supplied by `AsideDefaultProvider`, read by {@link useAsideDefault}. */
export const AsideDefaultContext = createContext<AsideDefaultCtx | null>(null)

export function useAsideDefault(): AsideDefaultCtx {
  const ctx = use(AsideDefaultContext)
  if (!ctx) throw new Error("useAsideDefault must be used within AsideDefaultProvider")
  return ctx
}
