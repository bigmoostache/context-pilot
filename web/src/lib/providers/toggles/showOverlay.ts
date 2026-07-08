import { createContext, use } from "react"

/**
 * Global **performance-overlay** preference (T514).
 *
 * Controls the corner-docked {@link TelemetryHud} readout. Previously the HUD
 * was gated behind Developer mode, which was too invasive — dev mode also
 * reveals the Cockpit/Costs tabs, so a user who only wanted the perf overlay
 * had to opt into the whole developer surface. This flag decouples the two:
 * the overlay has its own **Show Overlay** switch, persisted to `localStorage`
 * only (a pure client-side view preference, off by default).
 *
 * The provider component lives in `./ShowOverlayProvider` (split out so this
 * module exports no component, satisfying the Fast-Refresh purity rule).
 * Persisted under `cp-show-overlay`; any value other than the literal `"1"`
 * resolves to **off**, guaranteeing the safe default.
 */
export interface ShowOverlayCtx {
  showOverlay: boolean
  setShowOverlay: (on: boolean) => void
  toggle: () => void
}

/** Show-overlay context object. Supplied by `ShowOverlayProvider`, read by {@link useShowOverlay}. */
export const ShowOverlayContext = createContext<ShowOverlayCtx | null>(null)

export function useShowOverlay(): ShowOverlayCtx {
  const ctx = use(ShowOverlayContext)
  if (!ctx) throw new Error("useShowOverlay must be used within ShowOverlayProvider")
  return ctx
}
