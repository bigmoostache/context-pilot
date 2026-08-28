import { useCallback, useState } from "react"
import type { UploadedFile } from "./helpers"

/** Which tab of the {@link ThreadAside} is showing. */
export type ThreadAsideTab = "files" | "tasks"

/**
 * Owns the {@link ThreadAside} state (T662): which tab is active and the file
 * currently previewed in the Files tab. The returned `openFile` switches to the
 * Files tab showing a given attachment, so an in-message file chip drives the
 * same rail the tab bar does (replacing the old QuickLook drawer).
 *
 * Lives in its own module (not beside the component) so `ThreadAside.tsx` keeps
 * a component-only export surface — the react-refresh/only-export-components
 * invariant that keeps Fast Refresh working.
 */
export function useThreadAside() {
  const [tab, setTab] = useState<ThreadAsideTab>("tasks")
  const [file, setFile] = useState<UploadedFile | null>(null)
  const openFile = useCallback((f: UploadedFile) => {
    setFile(f)
    setTab("files")
  }, [])
  return { tab, setTab, file, setFile, openFile }
}
