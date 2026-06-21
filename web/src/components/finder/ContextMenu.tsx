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
  Tag,
  Trash2,
  Upload,
} from "lucide-react"
import type { FinderNode, FinderTag } from "@/lib/types"
import { TAG_META } from "./support/kind"

export interface MenuPos {
  x: number
  y: number
  /** the right-clicked node — ABSENT for an empty-space (content-area) click,
   *  which renders the realm-level menu (New Folder, Upload, …) instead. */
  node?: FinderNode
}

/**
 * Right-click context menu — a faithful macOS-style action sheet. Decorative
 * (actions flash a toast) but fully interactive, with the realm's tag swatches.
 */
export function ContextMenu({
  pos,
  onClose,
  onAction,
  onOpen,
  onDownload,
  onTag,
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
  /** Toggle a tag on a node. */
  onTag: (node: FinderNode, tag: FinderTag) => void
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

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
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
  const isFolder = node?.kind === "folder"
  // keep the menu on-screen
  const left = Math.min(pos.x, window.innerWidth - 230)
  const top = Math.min(pos.y, window.innerHeight - 320)

  const Item = ({
    icon: Icon,
    label,
    danger,
    shortcut,
  }: {
    icon: typeof Copy
    label: string
    danger?: boolean
    shortcut?: string
  }) => (
    <button
      onClick={() => {
        onAction(label)
        onClose()
      }}
      className={
        "flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] transition-colors " +
        (danger
          ? "text-[var(--danger)] hover:bg-[var(--danger)]/12"
          : "text-foreground/85 hover:bg-[var(--signal)]/14 hover:text-foreground")
      }
    >
      <Icon className="size-3.5 shrink-0 opacity-80" />
      <span className="flex-1">{label}</span>
      {shortcut && <span className="text-[10.5px] tabular-nums text-muted-foreground/50">{shortcut}</span>}
    </button>
  )

  return (
    <div
      ref={ref}
      className="menu-pop fixed z-50 w-[214px] rounded-xl border border-border bg-popover/95 p-1.5 backdrop-blur-xl pop-shadow"
      style={{ left, top }}
    >
      {!node ? (
        // ── Empty-space (content-area) menu — realm-level actions ──
        <>
          <EmptyItem
            icon={FolderPlus}
            label="New Folder"
            onClick={() => {
              onNewFolder()
              onClose()
            }}
            shortcut="⇧⌘N"
          />
          <EmptyItem
            icon={Upload}
            label="Upload…"
            onClick={() => {
              onUpload()
              onClose()
            }}
          />
          <Separator />
          <EmptyItem
            icon={CheckSquare}
            label="Select All"
            onClick={() => {
              onSelectAll()
              onClose()
            }}
            shortcut="⌘A"
          />
          <EmptyItem
            icon={PanelRight}
            label="Toggle Quick Look"
            onClick={() => {
              onTogglePreview()
              onClose()
            }}
            shortcut="Space"
          />
          <Separator />
          <EmptyItem
            icon={RefreshCw}
            label="Refresh"
            onClick={() => {
              onRefresh()
              onClose()
            }}
          />
        </>
      ) : (
        <>
          <button
            onClick={() => {
              onOpen(node)
              onClose()
            }}
            className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] text-foreground/85 transition-colors hover:bg-[var(--signal)]/14 hover:text-foreground"
          >
            <FolderOpen className="size-3.5 shrink-0 opacity-80" />
            <span className="flex-1">{isFolder ? "Open" : "Open Quick Look"}</span>
            <span className="text-[10.5px] tabular-nums text-muted-foreground/50">{isFolder ? "↵" : "Space"}</span>
          </button>

          {isFolder && onPin && (
            <button
              onClick={() => {
                onPin(node)
                onClose()
              }}
              className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] text-foreground/85 transition-colors hover:bg-[var(--signal)]/14 hover:text-foreground"
            >
              <Pin className="size-3.5 shrink-0 opacity-80" />
              <span className="flex-1">Pin to Sidebar</span>
            </button>
          )}

          <Separator />

          <button
            onClick={() => {
              onRenameStart(node)
              onClose()
            }}
            className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] text-foreground/85 transition-colors hover:bg-[var(--signal)]/14 hover:text-foreground"
          >
            <PencilLine className="size-3.5 shrink-0 opacity-80" />
            <span className="flex-1">Rename</span>
          </button>
          <Item icon={Copy} label="Duplicate" shortcut="⌘D" />
          <button
            onClick={() => {
              onDownload(node)
              onClose()
            }}
            className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] text-foreground/85 transition-colors hover:bg-[var(--signal)]/14 hover:text-foreground"
          >
            <Download className="size-3.5 shrink-0 opacity-80" />
            <span className="flex-1">Download</span>
          </button>

          <Separator />

          {/* tag swatches — the macOS finder signature */}
          <div className="flex items-center gap-1.5 px-2.5 py-1.5">
            <Tag className="size-3.5 shrink-0 text-muted-foreground/70" />
            <div className="flex flex-1 items-center justify-between">
              {Object.entries(TAG_META).map(([key, t]) => (
                <button
                  key={key}
                  title={t.label}
                  onClick={() => {
                    onTag(node, key as FinderTag)
                    onClose()
                  }}
                  className="size-3.5 rounded-full ring-1 ring-inset ring-black/10 transition-transform hover:scale-125"
                  style={{ background: t.color }}
                />
              ))}
            </div>
          </div>

          <Separator />

          <button
            onClick={() => {
              onTrash(node)
              onClose()
            }}
            className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] text-[var(--danger)] transition-colors hover:bg-[var(--danger)]/12"
          >
            <Trash2 className="size-3.5 shrink-0 opacity-80" />
            <span className="flex-1">Move to Trash</span>
            <span className="text-[10.5px] tabular-nums text-muted-foreground/50">⌘⌫</span>
          </button>
        </>
      )}
    </div>
  )
}

/** A plain icon+label row for the empty-space menu (no node, direct onClick). */
function EmptyItem({
  icon: Icon,
  label,
  onClick,
  shortcut,
}: {
  icon: typeof Copy
  label: string
  onClick: () => void
  shortcut?: string
}) {
  return (
    <button
      onClick={onClick}
      className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] text-foreground/85 transition-colors hover:bg-[var(--signal)]/14 hover:text-foreground"
    >
      <Icon className="size-3.5 shrink-0 opacity-80" />
      <span className="flex-1">{label}</span>
      {shortcut && <span className="text-[10.5px] tabular-nums text-muted-foreground/50">{shortcut}</span>}
    </button>
  )
}

function Separator() {
  return <div className="my-1 h-px bg-border/70" />
}
