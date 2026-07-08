import { useEffect, useRef, useState } from "react"
import type { FinderNode, FinderViewMode } from "@/lib/types"
import { useMarquee } from "../support/useMarquee"
import { CLICK_SETTLE_MS, loadPins, pinsKey, type PinnedFolder } from "./helpers"

/**
 * Peripheral Finder state hooks, split out of `Finder.tsx` (web-lint P6) so the
 * orchestration file stays under the 500-line structure cap after the a11y +
 * effect-hardening pass. Each hook owns one self-contained concern the main
 * component wired inline before; the behaviour is unchanged.
 */

/**
 * Per-agent pinned-folder list, persisted to localStorage. Seeds from
 * {@link loadPins} and re-writes on every change (a full/unavailable store is
 * swallowed — pins simply stay in-session). Returns the list plus idempotent
 * add/remove helpers.
 */
export function useFinderPins(agentId: string) {
  const [pins, setPins] = useState<PinnedFolder[]>(() => loadPins(agentId))

  useEffect(() => {
    try {
      localStorage.setItem(pinsKey(agentId), JSON.stringify(pins))
    } catch {
      /* storage full / unavailable — pins stay in-session only */
    }
  }, [pins, agentId])

  const addPin = (p: PinnedFolder) =>
    setPins((cur) => (cur.some((x) => x.path === p.path) ? cur : [...cur, p]))
  const removePin = (path: string) => setPins((cur) => cur.filter((x) => x.path !== path))

  return { pins, addPin, removePin }
}

interface RevealDeps {
  revealPath: string | null | undefined
  agentFolder: string
  navigate: (path: string) => void
  onRevealConsumed: (() => void) | undefined
  setSelected: (s: Set<string>) => void
  setFocusPath: (p: string | null) => void
}

/**
 * "Show in Finder" (T334) — navigate to a revealed file's parent and select it.
 * This reacts to a cross-component prop *signal* (App sets `revealPath` when the
 * user picks "Show in Finder" elsewhere), so an effect is the right tool. Two
 * honesty details keep it lint-clean without an inline disable (banned since
 * P4):
 *   • `navigate` + `onRevealConsumed` are recreated every render, so they're
 *     read through a latest-ref (refreshed by the assignment effect below)
 *     rather than listed as deps — listing them would re-run the reveal on every
 *     render; omitting them without the ref would be a stale closure. The honest
 *     deps are just [revealPath, agentFolder] plus the stable state setters.
 *   • the navigate + selection updates are deferred to a microtask so they run
 *     AFTER commit, not synchronously inside the effect — exactly the cascading
 *     render @eslint-react/set-state-in-effect guards against (and `navigate`
 *     itself reads a ref, so it can't run during render).
 */
export function useRevealPath(d: RevealDeps) {
  const fnsRef = useRef({ navigate: d.navigate, onRevealConsumed: d.onRevealConsumed })
  useEffect(() => {
    fnsRef.current = { navigate: d.navigate, onRevealConsumed: d.onRevealConsumed }
  })
  const { revealPath, agentFolder, setSelected, setFocusPath } = d
  useEffect(() => {
    if (!revealPath) return
    const lastSlash = revealPath.lastIndexOf("/")
    const parentRel = lastSlash === -1 ? "" : revealPath.slice(0, lastSlash)
    const parentAbs = parentRel ? `${agentFolder}/${parentRel}` : agentFolder
    queueMicrotask(() => {
      const { navigate, onRevealConsumed } = fnsRef.current
      navigate(parentAbs)
      setSelected(new Set([revealPath]))
      setFocusPath(revealPath)
      onRevealConsumed?.()
    })
  }, [revealPath, agentFolder, setSelected, setFocusPath])
}

/**
 * Mount-only surface behaviour: focus the keyboard surface so shortcuts work
 * immediately.
 */
export function useFinderMount(surfaceRef: React.RefObject<HTMLElement | null>) {
  useEffect(() => {
    surfaceRef.current?.focus()
  }, [surfaceRef])
}

/**
 * Single-click "settle" timer (see {@link CLICK_SETTLE_MS}). macOS-style rows
 * fire an instant select on click, then a DEFERRED slow-rename / non-reflowing
 * QuickLook after a short settle window — so a quick double-click / open /
 * navigate can pre-empt it. This hook owns the timer ref and clears it on
 * unmount; it hands back `arm`/`clear` CLOSURES rather than the ref, so the ref
 * never crosses a render-time function-call boundary into the Finder's handler
 * factories (@eslint-react/refs — a ref must not be read during render).
 */
export function useClickSettle() {
  const clickTimerRef = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(clickTimerRef.current), [])
  const armClickSettle = (fn: () => void) => {
    window.clearTimeout(clickTimerRef.current)
    clickTimerRef.current = window.setTimeout(fn, CLICK_SETTLE_MS)
  }
  const clearClickSettle = () => window.clearTimeout(clickTimerRef.current)
  return { armClickSettle, clearClickSettle }
}

/**
 * Box (rubber-band) selection wiring for the Finder main area. Marquee drag is
 * only meaningful in the flat grid / list layouts (columns + gallery own their
 * own interaction), so it's enabled for those two view modes; the returned
 * `marqueeOn` flag also drives the caller's empty-space click fallback and the
 * `select-none` class. Split out of Finder.tsx (web-lint P6, ≤500-line cap).
 */
export function useFinderMarquee(d: {
  viewMode: FinderViewMode
  getSelected: () => Set<string>
  onChange: (s: Set<string>) => void
  onClear: () => void
}) {
  const mainRef = useRef<HTMLElement>(null)
  const marqueeOn = d.viewMode === "grid" || d.viewMode === "list"
  const marquee = useMarquee({
    containerRef: mainRef,
    enabled: marqueeOn,
    getSelected: d.getSelected,
    onChange: d.onChange,
    onEmptyClick: d.onClear,
  })
  return { mainRef, marqueeOn, band: marquee.band, handlers: marquee.handlers }
}

/** Total byte size of the currently-selected nodes (Finder status-bar figure). */
export function finderSelectionSize(selected: Set<string>, children: FinderNode[]): number {
  return [...selected]
    .map((p) => children.find((c) => c.path === p))
    .reduce((sum, n) => sum + (n?.size ?? 0), 0)
}
