import { useCallback, useMemo, useState, type ReactNode } from "react"
import { TopButtonsContext } from "./index"

/**
 * Owns the top-corner-buttons coordination state (see {@link TopButtonsContext}).
 * Mount once above the mobile shell so every `CornerButton` shares one `navKey`;
 * a view calls `bump()` on each navigation edge (page switch, drawer toggle) to
 * make the corner controls re-spring their glyph in lock-step with the change.
 *
 * Split from its context/hook module so this file only exports a component
 * (react-refresh/only-export-components), matching the repo's provider-split
 * convention.
 */
export function TopButtonsProvider({ children }: { children: ReactNode }) {
  const [navKey, setNavKey] = useState(0)
  const bump = useCallback(() => setNavKey((k) => k + 1), [])
  const value = useMemo(() => ({ navKey, bump }), [navKey, bump])
  return <TopButtonsContext value={value}>{children}</TopButtonsContext>
}
