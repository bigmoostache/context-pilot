import { UploadCloud } from "lucide-react"
import { useQueryClient } from "@tanstack/react-query"
import type { FinderNode, FinderViewMode } from "@/lib/types"
import { fmtBytes } from "@/lib/support/finderFs"
import { downloadFile } from "@/lib/live"
import { qk } from "@/lib/query/sync"
import { ContextMenu, type MenuPos } from "../ContextMenu"
import { pathName, type PinnedFolder } from "./helpers"

interface OverlaysDeps {
  agentId: string
  relCwd: string
  itemCount: number
  selected: Set<string>
  selSize: number
  viewMode: FinderViewMode
  cwd: string
  sorted: FinderNode[]
  menu: MenuPos | null
  dragging: boolean
  toast: string | null
  flash: (msg: string) => void
  open: (node: FinderNode) => void
  addPin: (p: PinnedFolder) => void
  startRename: (node: FinderNode) => void
  trashNode: (node: FinderNode) => void
  newFolder: () => void
  pickFiles: () => void
  setSelected: (s: Set<string>) => void
  setPreviewOpen: (fn: (o: boolean) => boolean) => void
  setMenu: (m: MenuPos | null) => void
}

/**
 * The Finder's bottom chrome + floating layers: the status bar, the right-click
 * {@link ContextMenu} (with its full handler wiring), the external-drag upload
 * overlay, and the transient toast. Extracted from the component so the main
 * file stays under the size limit; it builds the context-menu closures from the
 * primitive deps it's given rather than receiving a dozen pre-bound callbacks.
 */
export function FinderOverlays(d: OverlaysDeps) {
  const qc = useQueryClient()
  return (
    <>
      {/* status bar */}
      <div className="flex h-7 shrink-0 items-center gap-3 border-t border-border bg-surface px-4 text-[11px] text-muted-foreground">
        <span>{d.itemCount} items</span>
        {d.selected.size > 0 && (
          <>
            <span className="h-3 w-px bg-border" />
            <span className="text-foreground/80">
              {d.selected.size} selected · {fmtBytes(d.selSize)}
            </span>
          </>
        )}
        <span className="ml-auto capitalize">{d.viewMode} view</span>
        <span className="h-3 w-px bg-border" />
        <span className="hidden sm:inline">128 GB available</span>
        <span className="h-3 w-px bg-border" />
        <span className="font-mono text-[10.5px]">{d.cwd}</span>
      </div>

      {/* context menu */}
      {d.menu && (
        <ContextMenu
          pos={d.menu}
          onClose={() => d.setMenu(null)}
          onAction={(label) => d.flash(label)}
          onOpen={d.open}
          onDownload={(n) => {
            downloadFile(d.agentId, n.path).catch(() => d.flash(`Failed to download ${n.name}`))
            d.flash(`Downloading ${n.name}…`)
          }}
          onTag={(_n, tag) => d.flash(`Tagged as ${tag}`)}
          onPin={(n) => {
            d.addPin({ name: n.name, path: n.path })
            d.flash(`Pinned ${n.name}`)
          }}
          onRenameStart={d.startRename}
          onTrash={d.trashNode}
          onNewFolder={d.newFolder}
          onUpload={d.pickFiles}
          onSelectAll={() => d.setSelected(new Set(d.sorted.map((n) => n.path)))}
          onTogglePreview={() => d.setPreviewOpen((o) => !o)}
          onRefresh={() => {
            void qc.invalidateQueries({ queryKey: qk.fs(d.agentId, d.relCwd) })
            d.flash("Refreshed")
          }}
        />
      )}

      {/* drag-drop overlay */}
      {d.dragging && (
        <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center bg-[var(--signal)]/8 backdrop-blur-[2px]">
          <div className="ants flex flex-col items-center gap-3 rounded-2xl bg-card/90 px-10 py-8 pop-shadow">
            <UploadCloud className="size-9 text-[var(--signal)]" />
            <span className="text-[14px] font-semibold text-foreground">Drop to upload</span>
            <span className="text-[12px] text-muted-foreground">into {pathName(d.cwd)}</span>
          </div>
        </div>
      )}

      {/* transient toast */}
      {d.toast && (
        <div className="absolute bottom-10 left-1/2 z-30 -translate-x-1/2 rounded-lg border border-border bg-card px-4 py-2 text-[12px] text-foreground/90 pop-shadow">
          {d.toast}
        </div>
      )}
    </>
  )
}
