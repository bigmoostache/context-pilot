import { createContext, use } from "react"

/**
 * Global **dev-mode** preference (T301).
 *
 * Dev mode is an app-wide UI toggle, **off by default**, that reveals
 * developer-facing surfaces the everyday user has no need for — currently the
 * **Cockpit** view tab (the agent's live context-panel inspector), which is
 * hidden from the TopBar unless dev mode is active.
 *
 * The provider component lives in `./DevModeProvider` (split out so this module
 * exports no component, satisfying the Fast-Refresh purity rule). Persisted
 * under `cp-dev-mode`; any value other than the literal `"1"` resolves to
 * **off**, guaranteeing the safe default.
 */
export interface DevModeCtx {
  devMode: boolean
  setDevMode: (on: boolean) => void
  toggle: () => void
}

/** Dev-mode context object. Supplied by `DevModeProvider`, read by {@link useDevMode}. */
export const DevModeContext = createContext<DevModeCtx | null>(null)

export function useDevMode(): DevModeCtx {
  const ctx = use(DevModeContext)
  if (!ctx) throw new Error("useDevMode must be used within DevModeProvider")
  return ctx
}
