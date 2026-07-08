import { useEffect, useState } from "react"
import { ShowOverlayContext } from "./showOverlay"

const STORAGE_KEY = "cp-show-overlay"

function initialShowOverlay(): boolean {
  if (typeof window === "undefined") return false
  return window.localStorage.getItem(STORAGE_KEY) === "1"
}

/** Provides the global show-overlay flag and persists it to `localStorage`. */
export function ShowOverlayProvider({ children }: { children: React.ReactNode }) {
  const [showOverlay, setShowOverlay] = useState<boolean>(initialShowOverlay)

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, showOverlay ? "1" : "0")
  }, [showOverlay])

  const toggle = () => setShowOverlay((v) => !v)

  return (
    <ShowOverlayContext value={{ showOverlay, setShowOverlay, toggle }}>
      {children}
    </ShowOverlayContext>
  )
}
