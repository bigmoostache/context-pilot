import { createContext, useContext, useEffect, useState } from "react"

/**
 * Global **dev-mode** preference (T301).
 *
 * Dev mode is an app-wide UI toggle, **off by default**, that reveals
 * developer-facing surfaces the everyday user has no need for — currently the
 * **Cockpit** view tab (the agent's live context-panel inspector), which is
 * hidden from the TopBar unless dev mode is active.
 *
 * It mirrors {@link "@/lib/theme".ThemeProvider} exactly: a context-backed
 * boolean persisted to `localStorage` so the choice survives reloads, exposed
 * through {@link useDevMode}. Persisted under `cp-dev-mode`; any value other
 * than the literal `"1"` (including a first-ever visit with no key) resolves to
 * **off**, guaranteeing the safe default.
 */
interface DevModeCtx {
  devMode: boolean
  setDevMode: (on: boolean) => void
  toggle: () => void
}

const Ctx = createContext<DevModeCtx | null>(null)

const STORAGE_KEY = "cp-dev-mode"

function initialDevMode(): boolean {
  if (typeof window === "undefined") return false
  return window.localStorage.getItem(STORAGE_KEY) === "1"
}

/** Provides the global dev-mode flag and persists it to `localStorage`. */
export function DevModeProvider({ children }: { children: React.ReactNode }) {
  const [devMode, setDevMode] = useState<boolean>(initialDevMode)

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, devMode ? "1" : "0")
  }, [devMode])

  const toggle = () => setDevMode((d) => !d)

  return <Ctx.Provider value={{ devMode, setDevMode, toggle }}>{children}</Ctx.Provider>
}

export function useDevMode(): DevModeCtx {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error("useDevMode must be used within DevModeProvider")
  return ctx
}
