import { useCallback, useEffect, useState } from "react"
import type { UploadedFile } from "./helpers"

/** Which tab of the {@link ThreadAside} is showing. */
export type ThreadAsideTab = "files" | "tasks"

/** localStorage key holding a thread's show/hide choice for its right aside. */
const hiddenKey = (agentId: string, threadId: string) => `cp-aside-hidden-${agentId}-${threadId}`

/**
 * Read a thread's persisted aside show/hide choice. `"1"` → hidden, `"0"` →
 * shown; absent → fall back to the global default (`defaultHidden`). Refresh-
 * robust: once a thread is toggled the choice is stored and survives reloads.
 */
function readHidden(agentId: string, threadId: string, defaultHidden: boolean): boolean {
  if (typeof window === "undefined") return defaultHidden
  const raw = window.localStorage.getItem(hiddenKey(agentId, threadId))
  if (raw === "1") return true
  if (raw === "0") return false
  return defaultHidden
}

/**
 * Owns the {@link ThreadAside} state (T662): which tab is active, the file
 * currently previewed in the Files tab, and — since T677 — whether the whole
 * rail is hidden for this thread. The returned `openFile` switches to the Files
 * tab showing a given attachment AND re-shows the rail if it was hidden, so an
 * in-message file chip both drives the same rail the tab bar does and undoes a
 * prior hide.
 *
 * The hidden flag is persisted per-thread (`cp-aside-hidden-<agent>-<thread>`)
 * with the global {@link useAsideDefault} value as the fallback for a thread
 * never toggled. `ThreadConversation` is NOT remounted on thread switch, so the
 * hidden state is re-seeded from storage during render whenever the
 * agent/thread key changes (React's "adjust state while rendering" pattern).
 *
 * Lives in its own module (not beside the component) so `ThreadAside.tsx` keeps
 * a component-only export surface — the react-refresh/only-export-components
 * invariant that keeps Fast Refresh working.
 */
export function useThreadAside(agentId: string, threadId: string, defaultHidden: boolean) {
  const [tab, setTab] = useState<ThreadAsideTab>("tasks")
  const [file, setFile] = useState<UploadedFile | null>(null)
  const [hidden, setHidden] = useState<boolean>(() =>
    readHidden(agentId, threadId, defaultHidden),
  )

  // Re-seed the per-thread hidden choice when the agent/thread key changes
  // (the component instance is reused across thread switches). Guarded by the
  // previous key so it fires exactly once per switch, not every render.
  const key = hiddenKey(agentId, threadId)
  const [prevKey, setPrevKey] = useState(key)
  if (prevKey !== key) {
    setPrevKey(key)
    setHidden(readHidden(agentId, threadId, defaultHidden))
  }

  // Persist the choice for the current thread whenever it changes.
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
