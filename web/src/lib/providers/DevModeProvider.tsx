import { useEffect, useState } from "react"
import { DevModeContext } from "./devMode"

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

  return <DevModeContext.Provider value={{ devMode, setDevMode, toggle }}>{children}</DevModeContext.Provider>
}
