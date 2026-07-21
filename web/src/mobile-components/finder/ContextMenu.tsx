import { useEffect, useRef } from "react"
import {
  CheckSquare,
  Copy,
  Download,
  FolderOpen,
  FolderPlus,
  PanelRight,
  PencilLine,
  Pin,
  RefreshCw,
  Trash2,
  Upload,
} from "lucide-react"
import type { FinderNode } from "@/lib/types"

export interface MenuPos {
  x: number
  y: number
  /** the long-pressed node — ABSENT for an empty-space (content-area) press,
   *  which renders the realm-level menu (New Folder, Upload, …) instead. On
   *  mobile the x/y coordinates are ignored: the sheet always anchors to the
   *  bottom edge (there is no cursor to anchor to on touch). */
  node?: FinderNode
}

/**
 * Mobile context menu — a bottom action sheet, the touch-native replacement for
 * the desktop cursor-anchored popup. Triggered by a long-press (there is no
 * right-click on touch), so it ignores the pointer x/y entirely and slides up
 * from the bottom edge over a dimming scrim. Rows are full-width, ≥44px tall
 * for the thumb, and drop the desktop keyboard-shortcut hints (no hardware
 * keyboard on a phone). Tapping the scrim or an action closes it.
 */
export function ContextMenu({
  pos,
  onClose,
  onAction,
  onOpen,
  onDownload,
  onTrash,
  onPin,
  onNewFolder,
  onUpload,
  onSelectAll,
  onTogglePreview,
  onRefresh,
  onRenameStart,
}: {
  pos: MenuPos
  onClose: () => void
  onAction: (label: string) => void
  /** Open a file (Quick Look) or navigate a folder. */
  onOpen: (node: FinderNode) => void
  /** Trigger a real file download. */
  onDownload: (node: FinderNode) => void
  /** move a node to the realm trash (item menu) */
  onTrash: (node: FinderNode) => void
  /** pin a folder to the sidebar (folders only) */
  onPin?: (node: FinderNode) => void
  /** begin inline-renaming a node (item menu) */
  onRenameStart: (node: FinderNode) => void
  /** create a new folder in the current directory (empty-space menu) */
  onNewFolder: () => void
  /** open the file picker to upload into the current directory (empty-space menu) */
  onUpload: () => void
  /** select every item in the current directory (empty-space menu) */
  onSelectAll: () => void
  /** toggle the Quick Look preview pane (empty-space menu) */
  onTogglePreview: () => void
  /** re-fetch the current directory listing (empty-space menu) */
  onRefresh: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)

  // Outside-press + Esc close (mirrors the desktop ContextMenu): a press whose
  // target is outside the sheet dismisses it, so the full-screen scrim needs no
  // click handler (which would be a non-interactive-element a11y violation), and
  // Esc closes for a paired hardware keyboard on a tablet.
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && e.target instanceof Node && !ref.current.contains(e.target)) onClose()
    }
    const onEsc = (e: KeyboardEvent) => e.key === "Escape" && onClose()
    window.addEventListener("mousedown", onDown)
    window.addEventListener("keydown", onEsc)
    return () => {
      window.removeEventListener("mousedown", onDown)
      window.removeEventListener("keydown", onEsc)
    }
  }, [onClose])

  const node = pos.node

  return (
    // Full-screen scrim + bottom-anchored sheet. The scrim is a pure visual
    // backdrop; dismissal is handled by the outside-press listener above (a
    // press outside the sheet ref closes), so no div carries a click handler.
    <div className="menu-scrim fixed inset-0 z-50 flex flex-col justify-end bg-black/40 backdrop-blur-[1px]">
      <div
        ref={ref}
        className="menu-pop pop-shadow max-h-[80vh] overflow-y-auto rounded-t-2xl border-t border-border bg-popover/95 pb-[env(safe-area-inset-bottom)] backdrop-blur-xl"
      >
        {/* grabber handle */}
        <div className="flex justify-center pt-2 pb-1">
          <span className="h-1 w-9 rounded-full bg-muted-foreground/30" />
        </div>
        <div className="p-2">
          {node ? (
            <ItemMenu
              node={node}
              onClose={onClose}
              onAction={onAction}
              onOpen={onOpen}
              onDownload={onDownload}
              onTrash={onTrash}
              onPin={onPin}
              onRenameStart={onRenameStart}
            />
          ) : (
            <EmptyMenu
              onClose={onClose}
              onNewFolder={onNewFolder}
              onUpload={onUpload}
              onSelectAll={onSelectAll}
              onTogglePreview={onTogglePreview}
              onRefresh={onRefresh}
            />
          )}
        </div>
      </div>
    </div>
  )
}

/** The item (long-pressed node) menu: Open/Quick-Look, Pin (folders), Rename,
 *  Duplicate, Download, and the danger Move-to-Trash action. */
function ItemMenu({
  node,
  onClose,
  onAction,
  onOpen,
  onDownload,
  onTrash,
  onPin,
  onRenameStart,
}: {
  node: FinderNode
  onClose: () => void
  onAction: (label: string) => void
  onOpen: (node: FinderNode) => void
  onDownload: (node: FinderNode) => void
  onTrash: (node: FinderNode) => void
  onPin?: ((node: FinderNode) => void) | undefined
  onRenameStart: (node: FinderNode) => void
}) {
  const isFolder = node.kind === "folder"
  return (
    <>
      <Row
        icon={FolderOpen}
        label={isFolder ? "Open" : "Open Quick Look"}
        onClick={() => {
          onOpen(node)
          onClose()
        }}
      />
      {isFolder && onPin && (
        <Row
          icon={Pin}
          label="Pin to Sidebar"
          onClick={() => {
            onPin(node)
            onClose()
          }}
        />
      )}
      <Separator />
      <Row
        icon={PencilLine}
        label="Rename"
        onClick={() => {
          onRenameStart(node)
          onClose()
        }}
      />
      <Row
        icon={Copy}
        label="Duplicate"
        onClick={() => {
          onAction("Duplicate")
          onClose()
        }}
      />
      <Row
        icon={Download}
        label="Download"
        onClick={() => {
          onDownload(node)
          onClose()
        }}
      />
      <Separator />
      <Row
        icon={Trash2}
        label="Move to Trash"
        danger
        onClick={() => {
          onTrash(node)
          onClose()
        }}
      />
    </>
  )
}

/** The empty-space (content-area) menu: realm-level actions — New Folder,
 *  Upload, Select All, Toggle Quick Look, Refresh. */
function EmptyMenu({
  onClose,
  onNewFolder,
  onUpload,
  onSelectAll,
  onTogglePreview,
  onRefresh,
}: {
  onClose: () => void
  onNewFolder: () => void
  onUpload: () => void
  onSelectAll: () => void
  onTogglePreview: () => void
  onRefresh: () => void
}) {
  return (
    <>
      <Row
        icon={FolderPlus}
        label="New Folder"
        onClick={() => {
          onNewFolder()
          onClose()
        }}
      />
      <Row
        icon={Upload}
        label="Upload…"
        onClick={() => {
          onUpload()
          onClose()
        }}
      />
      <Separator />
      <Row
        icon={CheckSquare}
        label="Select All"
        onClick={() => {
          onSelectAll()
          onClose()
        }}
      />
      <Row
        icon={PanelRight}
        label="Toggle Quick Look"
        onClick={() => {
          onTogglePreview()
          onClose()
        }}
      />
      <Separator />
      <Row
        icon={RefreshCw}
        label="Refresh"
        onClick={() => {
          onRefresh()
          onClose()
        }}
      />
    </>
  )
}

/** A full-width ≥44px touch action row. Hoisted to module scope (not defined
 *  inside ContextMenu) so React never treats it as a fresh component per render
 *  (@eslint-react/no-nested-component-definitions); every callback it uses is an
 *  explicit prop. Active-press feedback replaces the desktop hover state. */
function Row({
  icon: Icon,
  label,
  danger,
  onClick,
}: {
  icon: typeof Copy
  label: string
  danger?: boolean
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      className={
        "flex w-full items-center gap-3 rounded-lg p-3 text-left text-[15px] transition-colors " +
        (danger
          ? "text-(--danger) active:bg-(--danger)/12"
          : "text-foreground/90 active:bg-(--signal)/14")
      }
    >
      <Icon className="size-4.5 shrink-0 opacity-80" />
      <span className="flex-1">{label}</span>
    </button>
  )
}

function Separator() {
  return <div className="my-1 h-px bg-border/70" />
}
