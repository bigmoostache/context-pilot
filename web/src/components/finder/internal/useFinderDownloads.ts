import type { FinderNode } from "@/lib/types"
import { downloadFile } from "@/lib/live"

interface DownloadDeps {
  agentId: string
  /** live listing of the current directory (selection paths resolve against it) */
  children: FinderNode[]
  /** realm-relative paths of the currently selected entries */
  selected: Set<string>
  /** the single file a file-tab is showing, or null on a folder tab */
  activeFileNode: FinderNode | null
  flash: (msg: string) => void
}

/**
 * The Finder's download actions, split out of `Finder.tsx` (web-lint P2) so the
 * orchestration file stays under the 500-line structure cap after the Prettier
 * reflow. Both handlers stream files straight from the orchestration plane via
 * {@link downloadFile}; failures surface through the shared `flash` toast rather
 * than throwing.
 */
export function useFinderDownloads(d: DownloadDeps) {
  // Toolbar "Download": stream every selected *file* (folders are skipped —
  // there's no archive-a-folder affordance here). No selection, or a
  // folders-only selection, is a no-op with an explanatory toast.
  const downloadSelected = () => {
    if (d.selected.size === 0) {
      d.flash("Select files to download.")
      return
    }
    const files = [...d.selected]
      .map((p) => d.children.find((c) => c.path === p))
      .filter((n) => n && n.kind !== "folder")
    if (files.length === 0) {
      d.flash("Select files (not folders) to download.")
      return
    }
    for (const node of files) {
      if (node)
        downloadFile(d.agentId, node.path).catch(() => d.flash(`Failed to download ${node.name}`))
    }
    d.flash(`Downloading ${files.length} file(s)…`)
  }

  // The file-tab toolbar's lone Download button: stream the one file the tab is
  // previewing. A no-op on a folder tab (guarded by `activeFileNode`).
  const downloadActiveFile = () => {
    const node = d.activeFileNode
    if (!node) return
    downloadFile(d.agentId, node.path).catch(() => d.flash("Download failed"))
    d.flash(`Downloading ${node.name}…`)
  }

  return { downloadSelected, downloadActiveFile }
}
