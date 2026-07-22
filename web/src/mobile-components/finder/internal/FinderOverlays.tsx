import { useQueryClient } from "@tanstack/react-query"
import type { FinderNode, FinderViewMode } from "@/lib/types"
import { fmtBytes } from "@/lib/support/finderFs"
import { downloadFile } from "@/lib/live"
import { qk } from "@/lib/query/sync"
import { ContextMenu, type MenuPos } from "../ContextMenu"
import { type PinnedFolder } from "./helpers"

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
 * The mobile Finder's floating layers: a compact status line and the long-press
 * {@link ContextMenu} (rendered as a bottom action sheet) with its full handler
 * wiring, plus the transient toast. The desktop external-drag upload overlay is
 * DROPPED — a phone has no file-drag gesture, so `dragging` is never true and
 * the overlay would be dead markup (uploads happen through the menu's Upload
 * action + the file picker instead). Like its desktop twin it builds the
 * context-menu closures from the primitive deps it's given.
 */
export function FinderOverlays(d: OverlaysDeps) {
  const qc = useQueryClient()
  return (
    <>
      {/* compact status bar */}
      <div className="flex h-8 shrink-0 items-center gap-2.5 border-t border-border bg-surface px-3 text-[11px] text-muted-foreground">
        <span>{d.itemCount} items</span>
        {d.selected.size > 0 && (
          <>
            <span className="h-3 w-px bg-border" />
            <span className="text-foreground/80">
              {d.selected.size} · {fmtBytes(d.selSize)}
            </span>
          </>
        )}
        <span className="ml-auto capitalize">{d.viewMode}</span>
      </div>

      {/* context menu — bottom action sheet */}
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

      {/* transient toast */}
      {d.toast && (
        <div className="pop-shadow absolute bottom-14 left-1/2 z-30 -translate-x-1/2 rounded-lg border border-border bg-card px-4 py-2 text-[12px] text-foreground/90">
          {d.toast}
        </div>
      )}
    </>
  )
}
