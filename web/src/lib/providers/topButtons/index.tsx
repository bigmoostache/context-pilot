// ── Top-corner buttons coordination context ──────────────────────────
//
// The mobile chrome renders "always-reachable" controls pinned to the top-left
// and top-right screen corners (the shared `CornerButton` primitive — drawer
// toggle, agents grid, agent settings, archived toggle). Each view mounts its
// own corner buttons, so as you navigate (page switch, drawer open/close) the
// button in a given corner CHANGES its glyph and action.
//
// This context is the single coordination point for the transition between
// those changing buttons: it exposes a monotonically-increasing `navKey` that
// callers bump on every navigation edge. Each mounted `CornerButton` reads
// `navKey` and, when it changes, re-fires its icon-swap spring — so a page
// change animates the corner controls in lock-step instead of a hard cut.
//
// The context + hook live here (no component export, so react-refresh stays
// happy); the `TopButtonsProvider` component that owns the state lives in its
// sibling `TopButtonsProvider.tsx`, matching the repo's provider-split
// convention. Being pure logic it needs no mobile-mirror twin — the
// presentation (the glass circular button + the anime.js springs) stays in
// `CornerButton`.

import { createContext, use } from "react"

/** The coordination surface every `CornerButton` reads. */
export interface TopButtonsApi {
  /** Bumps once per navigation edge (view switch, drawer toggle). CornerButton
   *  re-runs its icon-swap spring whenever this changes. */
  navKey: number
  /** Signal a navigation edge — call from a view when its corner buttons are
   *  about to change (different glyph / action for the same corner). */
  bump: () => void
}

// Default is a no-op context so a `CornerButton` rendered OUTSIDE a provider
// (e.g. an isolated test, or the desktop tree that never mounts the provider)
// still works — it simply gets a static navKey and never re-springs on nav.
// `bump` is an explicit no-op (a body-less arrow trips no-empty-function).
const noop = (): void => undefined
export const TopButtonsContext = createContext<TopButtonsApi>({ navKey: 0, bump: noop })

/** Read the top-buttons coordination context (navKey + bump). Safe to call
 *  without a provider — returns a static no-op context. */
export function useTopButtons(): TopButtonsApi {
  return use(TopButtonsContext)
}
