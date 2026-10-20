import { useCallback, useEffect, useState } from "react"
import type { UploadedFile } from "./helpers"

/** Which tab of the {@link AgentAside} is showing. */
export type AgentAsideTab = "files" | "tasks" | "notes"

const TABS: readonly AgentAsideTab[] = ["files", "tasks", "notes"]

/** localStorage key holding a agent's show/hide choice for its right aside. */
const hiddenKey = (fleetId: string, agentId: string) => `cp-aside-hidden-${fleetId}-${agentId}`

/** localStorage key holding a agent's active aside tab + previewed file. */
const viewKey = (fleetId: string, agentId: string) => `cp-aside-view-${fleetId}-${agentId}`

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

/** The persisted aside view: active tab + the file open in the Files tab. */
interface AsideView {
  tab: AgentAsideTab
  file: UploadedFile | null
}

const DEFAULT_VIEW: AsideView = { tab: "tasks", file: null }

/** Narrow a parsed value to an {@link UploadedFile}; anything else → null. */
function asFile(v: unknown): UploadedFile | null {
  if (typeof v !== "object" || v === null) return null
  const { path, name, size, note } = v as Record<string, unknown>
  if (typeof path !== "string" || typeof name !== "string") return null
  return {
    path,
    name,
    size: typeof size === "number" ? size : 0,
    note: typeof note === "string" ? note : "",
  }
}

/**
 * Read a agent's persisted aside view. Tolerant: absent, malformed JSON or an
 * unknown tab falls back to {@link DEFAULT_VIEW} (tasks, no preview).
 */
function readView(fleetId: string, agentId: string): AsideView {
  if (typeof window === "undefined") return DEFAULT_VIEW
  const raw = window.localStorage.getItem(viewKey(fleetId, agentId))
  if (raw === null) return DEFAULT_VIEW
  try {
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== "object" || parsed === null) return DEFAULT_VIEW
    const { tab, file } = parsed as { tab?: unknown; file?: unknown }
    const knownTab = TABS.find((t) => t === tab)
    return { tab: knownTab ?? DEFAULT_VIEW.tab, file: asFile(file) }
  } catch {
    return DEFAULT_VIEW
  }
}

/**
 * Owns the {@link AgentAside} state (T662): which tab is active, the file
 * currently previewed in the Files tab, and — since T677 — whether the whole
 * rail is hidden for this agent. The returned `openFile` switches to the Files
 * tab showing a given attachment AND re-shows the rail if it was hidden, so an
 * in-message file chip both drives the same rail the tab bar does and undoes a
 * prior hide.
 *
 * All three are persisted per fleet+agent so leaving an agent (or its fleet)
 * and coming back restores the rail as it was: the hidden flag under
 * `cp-aside-hidden-<fleet>-<agent>` (falling back to the global
 * {@link useAsideDefault} value for a agent never toggled), tab + file under
 * `cp-aside-view-<fleet>-<agent>`. `AgentConversation` is NOT remounted on
 * agent switch, so the state is re-seeded from storage during render whenever
 * the fleet/agent key changes (React's "adjust state while rendering" pattern).
 *
 * Lives in its own module (not beside the component) so `AgentAside.tsx` keeps
 * a component-only export surface — the react-refresh/only-export-components
 * invariant that keeps Fast Refresh working.
 */
export function useAgentAside(fleetId: string, agentId: string, defaultHidden: boolean) {
  const [tab, setTab] = useState<AgentAsideTab>(() => readView(fleetId, agentId).tab)
  const [file, setFile] = useState<UploadedFile | null>(() => readView(fleetId, agentId).file)
  const [hidden, setHidden] = useState<boolean>(() => readHidden(fleetId, agentId, defaultHidden))

  // Re-seed the per-agent state when the fleet/agent key changes (the
  // component instance is reused across agent switches). Guarded by the
  // previous key so it fires exactly once per switch, not every render.
  const key = hiddenKey(fleetId, agentId)
  const vKey = viewKey(fleetId, agentId)
  const [prevKey, setPrevKey] = useState(key)
  if (prevKey !== key) {
    setPrevKey(key)
    setHidden(readHidden(fleetId, agentId, defaultHidden))
    const view = readView(fleetId, agentId)
    setTab(view.tab)
    setFile(view.file)
  }

  // Persist the choices for the current agent whenever they change. Skipped
  // while a re-seed is pending (prevKey lags key) so the previous agent's
  // values are never written under the new agent's key.
  const seeded = prevKey === key
  useEffect(() => {
    if (seeded) window.localStorage.setItem(key, hidden ? "1" : "0")
  }, [seeded, key, hidden])
  useEffect(() => {
    if (seeded) window.localStorage.setItem(vKey, JSON.stringify({ tab, file }))
  }, [seeded, vKey, tab, file])

  const openFile = useCallback((f: UploadedFile) => {
    setFile(f)
    setTab("files")
    setHidden(false) // a file click always re-shows the rail (T677)
  }, [])

  return { tab, setTab, file, setFile, hidden, setHidden, openFile }
}
