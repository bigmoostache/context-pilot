import { createContext, use } from "react"

export type Theme = "light" | "dark"

export interface ThemeCtx {
  theme: Theme
  setTheme: (t: Theme) => void
  toggle: () => void
}

/**
 * Theme context object. Consumed by {@link useTheme}; supplied by the
 * `ThemeProvider` component in `./ThemeProvider` (split out so this module
 * exports no component — the Fast-Refresh purity rule requires hooks and
 * components to live in separate files).
 */
export const ThemeContext = createContext<ThemeCtx | null>(null)

export function useTheme(): ThemeCtx {
  const ctx = use(ThemeContext)
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider")
  return ctx
}
