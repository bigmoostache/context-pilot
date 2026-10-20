// ── Browser Back / Next for the desktop shell (T636) ────────────────
//
// Extracted from components/Root.tsx (500-line cap). Pure History API wiring,
// no component imports, so it lives in shared lib and needs no mobile twin.

import { useEffect, useRef } from "react"
import type { ViewMode } from "@/lib/types"

/** The navigation atoms browser Back / Next restore (T636). One degree finer
 *  than view+agent: the accessed THREAD (agents view) rides along, so Back
 *  steps through the pages actually visited.
 *  Coarser below that (finder tab/split stays in localStorage; rail toggles are
 *  never entries). `agentId` is optional — only the field relevant to
 *  `view` is recorded, and an entry lacking it decodes fine. */
export interface NavState {
  view: ViewMode
  fleetId: string
  agentId?: string | undefined
}

/** Narrow the `any`-typed `history.state` / `PopStateEvent.state` down to a
 *  {@link NavState}, containing the `any` at this one boundary so no unsafe
 *  access leaks into the hook. A shape that isn't ours (a foreign history entry,
 *  or none) yields undefined. `view` is asserted after the string check — an
 *  unknown value just falls through the view router's fleet fallback, so it
 *  needs no exhaustive membership test. */
function readNav(state: unknown): NavState | undefined {
  if (typeof state !== "object" || state === null) return undefined
  const nav: unknown = (state as { nav?: unknown }).nav
  if (typeof nav !== "object" || nav === null) return undefined
  const { view, fleetId, agentId } = nav as {
    view?: unknown
    fleetId?: unknown
    agentId?: unknown
  }
  if (typeof view !== "string" || typeof fleetId !== "string") return undefined
  return {
    view: view as ViewMode,
    fleetId,
    agentId: typeof agentId === "string" ? agentId : undefined,
  }
}

/** Whether two nav entries point at the same surface — the dedupe test that
 *  keeps a redundant push (which would make one Back a no-op) off the stack. */
function sameNav(a: NavState, b: NavState): boolean {
  return a.view === b.view && a.fleetId === b.fleetId && a.agentId === b.agentId
}

/**
 * Wire browser Back / Next to the (view, agent, agent) tuple via
 * the History API directly — the app has no router and needs none here.
 *
 * Three effects: (1) PUSH on change — each tuple change pushes a state-only
 * entry, except a popstate echo (loop guard) or a push matching the top entry;
 * the first run `replaceState`s so the app starts with no duplicate base entry.
 * (2) APPLY on popstate — hands the entry's `nav` to `apply`, setting the atoms.
 * (3) LOOP GUARD — `applyingRef` swallows the one push-effect run that applying
 * a popstate triggers, so the entry just navigated to isn't re-pushed (same
 * one-shot-ref trick the SSE reducers use).
 *
 * URLs stay untouched: a hard refresh falls back to the localStorage-persisted
 * view + agent, the pre-existing behaviour.
 */
export function useNavHistory(
  current: {
    view: ViewMode
    fleetId: string
    agentId: string | undefined
  },
  apply: (s: NavState) => void,
) {
  const { view, fleetId, agentId } = current
  const applyingRef = useRef(false)
  const bootRef = useRef(false)

  useEffect(() => {
    // A popstate echo: the atoms changed because we just applied a history
    // entry, so there is nothing new to record. Swallow exactly one run.
    if (applyingRef.current) {
      applyingRef.current = false
      return
    }
    // Record only the atom relevant to the current view: a agents entry
    // carries its agent and every other view carries none, so a stale agent
    // never leaks into a settings/finder entry (which would break both the
    // dedupe test and the restore).
    const nav: NavState = {
      view,
      fleetId,
      ...(view === "agents" && agentId && { agentId }),
    }
    if (!bootRef.current) {
      // Seed the current location as the base entry rather than pushing a
      // second one on top of it.
      bootRef.current = true
      history.replaceState({ nav }, "")
      return
    }
    const cur = readNav(history.state)
    if (cur && sameNav(cur, nav)) return
    history.pushState({ nav }, "")
  }, [view, fleetId, agentId])

  useEffect(() => {
    const onPop = (e: PopStateEvent) => {
      const nav = readNav(e.state)
      if (!nav) return
      // Mark the coming push-effect run as a popstate echo (mechanism 3).
      applyingRef.current = true
      apply(nav)
    }
    window.addEventListener("popstate", onPop)
    return () => window.removeEventListener("popstate", onPop)
  }, [apply])
}
