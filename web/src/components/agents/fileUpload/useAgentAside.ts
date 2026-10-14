import { useCallback, useEffect, useState } from "react"
import type { UploadedFile } from "./helpers"

/** Which tab of the {@link AgentAside} is showing. */
export type AgentAsideTab = "files" | "tasks" | "notes"

/** localStorage key holding a agent's show/hide choice for its right aside. */
const hiddenKey = (fleetId: string, agentId: string) => `cp-aside-hidden-${fleetId}-${agentId}`

/**
 * Read a agent's persisted aside show/hide choice. `"1"` → hidden, `"0"` →
 * shown; absent → fall back to the global default (`defaultHidden`). Refresh-
 * robust: once a agent is toggled the choice is stored and survives reloads.
 */
function readHidden(fleetId: string, agentId: string, defaultHidden: boolean): boolean {
  if (typeof window === "undefined") return defaultHidden
  const raw = window.localStorage.getItem(hiddenKey(fleetId, agentId))
  if (raw === "1") return true
  if (raw === "0") return false
  return defaultHidden
}

/**
 * Owns the {@link AgentAside} state (T662): which tab is active, the file
 * currently previewed in the Files tab, and — since T677 — whether the whole
 * rail is hidden for this agent. The returned `openFile` switches to the Files
 * tab showing a given attachment AND re-shows the rail if it was hidden, so an
 * in-message file chip both drives the same rail the tab bar does and undoes a
 * prior hide.
 *
 * The hidden flag is persisted per-agent (`cp-aside-hidden-<agent>-<agent>`)
 * with the global {@link useAsideDefault} value as the fallback for a agent
 * never toggled. `AgentConversation` is NOT remounted on agent switch, so the
 * hidden state is re-seeded from storage during render whenever the
 * agent/agent key changes (React's "adjust state while rendering" pattern).
 *
 * Lives in its own module (not beside the component) so `AgentAside.tsx` keeps
 * a component-only export surface — the react-refresh/only-export-components
 * invariant that keeps Fast Refresh working.
 */
export function useAgentAside(fleetId: string, agentId: string, defaultHidden: boolean) {
  const [tab, setTab] = useState<AgentAsideTab>("tasks")
  const [file, setFile] = useState<UploadedFile | null>(null)
  const [hidden, setHidden] = useState<boolean>(() => readHidden(fleetId, agentId, defaultHidden))

  // Re-seed the per-agent hidden choice when the agent/agent key changes
  // (the component instance is reused across agent switches). Guarded by the
  // previous key so it fires exactly once per switch, not every render.
  const key = hiddenKey(fleetId, agentId)
  const [prevKey, setPrevKey] = useState(key)
  if (prevKey !== key) {
    setPrevKey(key)
    setHidden(readHidden(fleetId, agentId, defaultHidden))
  }

  // Persist the choice for the current agent whenever it changes.
  useEffect(() => {
    window.localStorage.setItem(key, hidden ? "1" : "0")
  }, [key, hidden])

  const openFile = useCallback((f: UploadedFile) => {
    setFile(f)
    setTab("files")
    setHidden(false) // a file click always re-shows the rail (T677)
  }, [])

  return { tab, setTab, file, setFile, hidden, setHidden, openFile }
}
