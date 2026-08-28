import { useEffect, useState } from "react"
import { AsideDefaultContext } from "./asideDefault"

const STORAGE_KEY = "cp-aside-default-hidden"

function initialDefaultHidden(): boolean {
  if (typeof window === "undefined") return false
  return window.localStorage.getItem(STORAGE_KEY) === "1"
}

/** Provides the global default-hidden flag for the thread aside and persists it
 *  to `localStorage` (`"1"` = hidden by default; default show). */
export function AsideDefaultProvider({ children }: { children: React.ReactNode }) {
  const [defaultHidden, setDefaultHidden] = useState<boolean>(initialDefaultHidden)

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, defaultHidden ? "1" : "0")
  }, [defaultHidden])

  const toggle = () => setDefaultHidden((v) => !v)

  return (
    <AsideDefaultContext value={{ defaultHidden, setDefaultHidden, toggle }}>
      {children}
    </AsideDefaultContext>
  )
}
