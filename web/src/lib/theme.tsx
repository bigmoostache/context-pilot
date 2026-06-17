import { createContext, useContext, useEffect, useState } from "react"

export type Theme = "light" | "dark"

interface ThemeCtx {
  theme: Theme
  setTheme: (t: Theme) => void
  toggle: () => void
}

const Ctx = createContext<ThemeCtx | null>(null)

const STORAGE_KEY = "cp-theme"

function initialTheme(): Theme {
  if (typeof window === "undefined") return "light"
  const saved = window.localStorage.getItem(STORAGE_KEY)
  if (saved === "light" || saved === "dark") return saved
  return "light"
}

/** Provides the active palette and applies the `.dark` class to <html>. */
export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setTheme] = useState<Theme>(initialTheme)

  useEffect(() => {
    const root = document.documentElement
    root.classList.toggle("dark", theme === "dark")
    window.localStorage.setItem(STORAGE_KEY, theme)
  }, [theme])

  const toggle = () => setTheme((t) => (t === "light" ? "dark" : "light"))

  return <Ctx.Provider value={{ theme, setTheme, toggle }}>{children}</Ctx.Provider>
}

export function useTheme(): ThemeCtx {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider")
  return ctx
}
