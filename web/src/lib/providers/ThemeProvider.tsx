import { useEffect, useState } from "react"
import { ThemeContext, type Theme } from "./theme"

const STORAGE_KEY = "cp-theme"

function initialTheme(): Theme {
  if (typeof window === "undefined") return "light"
  const saved = window.localStorage.getItem(STORAGE_KEY)
  if (saved === "light" || saved === "dark") return saved
  // No explicit choice yet: honour the OS preference (matches the pre-paint
  // init script in index.html so the first render agrees with the DOM class).
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light"
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

  return <ThemeContext value={{ theme, setTheme, toggle }}>{children}</ThemeContext>
}
