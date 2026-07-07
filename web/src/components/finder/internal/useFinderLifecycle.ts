import { useEffect } from "react"
import type { RefObject } from "react"

interface LifecycleDeps {
  surfaceRef: RefObject<HTMLDivElement | null>
  clickTimer: RefObject<number | undefined>
  revealPath?: string | null
  agentFolder: string
  navigate: (path: string) => void
  setSelected: (s: Set<string>) => void
  setFocusPath: (p: string | null) => void
  onRevealConsumed?: () => void
}

/**
 * The Finder surface's lifecycle effects: focus-on-mount (so keys work), a
 * single-click settle-timer cleanup on unmount, and the T334 "Show in Finder"
 * reveal — navigate to a file's parent and select it whenever `revealPath`
 * changes. <Finder> is re-mounted per agent via `key={agent.id}`, so the mount
 * effects run once per agent and `agentFolder`/`navigate` are stable within it.
 */
export function useFinderLifecycle({
  surfaceRef,
  clickTimer,
  revealPath,
  agentFolder,
  navigate,
  setSelected,
  setFocusPath,
  onRevealConsumed,
}: LifecycleDeps) {
  // focus the surface on mount + when the agent changes, so keys work
  useEffect(() => {
    surfaceRef.current?.focus()
  }, [surfaceRef])

  // Cancel any pending single-click settle timer on unmount.
  useEffect(() => () => window.clearTimeout(clickTimer.current), [clickTimer])

  // T334: "Show in Finder" — navigate to a file's parent and select it.
  useEffect(() => {
    if (!revealPath) return
    const lastSlash = revealPath.lastIndexOf("/")
    const parentRel = lastSlash >= 0 ? revealPath.slice(0, lastSlash) : ""
    const parentAbs = parentRel ? `${agentFolder}/${parentRel}` : agentFolder
    navigate(parentAbs)
    // Reveal is a one-shot navigation reacting to a new `revealPath`; selecting +
    // focusing the target is the point of the effect. The deps are intentionally
    // revealPath-only.
    setSelected(new Set([revealPath]))
    setFocusPath(revealPath)
    onRevealConsumed?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revealPath])
}
