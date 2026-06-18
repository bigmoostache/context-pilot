import { useEffect, useRef } from "react"
import {
  Copy,
  Download,
  FolderOpen,
  Info,
  PencilLine,
  Pin,
  Tag,
  Trash2,
} from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { TAG_META } from "./kind"

export interface MenuPos {
  x: number
  y: number
  node: FinderNode
}

/**
 * Right-click context menu — a faithful macOS-style action sheet. Decorative
 * (actions flash a toast) but fully interactive, with the realm's tag swatches.
 */
export function ContextMenu({
  pos,
  onClose,
  onAction,
  onGetInfo,
  onPin,
}: {
  pos: MenuPos
  onClose: () => void
  onAction: (label: string) => void
  onGetInfo: (node: FinderNode) => void
  /** pin a folder to the sidebar (folders only) */
  onPin?: (node: FinderNode) => void
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

  const isFolder = pos.node.kind === "folder"
  // keep the menu on-screen
  const left = Math.min(pos.x, window.innerWidth - 230)
  const top = Math.min(pos.y, window.innerHeight - 320)

  const Item = ({
    icon: Icon,
    label,
    danger,
    shortcut,
  }: {
    icon: typeof Info
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
      <Item icon={FolderOpen} label={isFolder ? "Open" : "Open Quick Look"} shortcut={isFolder ? "↵" : "Space"} />
      <button
        onClick={() => {
          onGetInfo(pos.node)
          onClose()
        }}
        className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] text-foreground/85 transition-colors hover:bg-[var(--signal)]/14 hover:text-foreground"
      >
        <Info className="size-3.5 shrink-0 opacity-80" />
        <span className="flex-1">Get Info</span>
        <span className="text-[10.5px] tabular-nums text-muted-foreground/50">⌘I</span>
      </button>

      {isFolder && onPin && (
        <button
          onClick={() => {
            onPin(pos.node)
            onClose()
          }}
          className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[12.5px] text-foreground/85 transition-colors hover:bg-[var(--signal)]/14 hover:text-foreground"
        >
          <Pin className="size-3.5 shrink-0 opacity-80" />
          <span className="flex-1">Pin to Sidebar</span>
        </button>
      )}

      <Separator />

      <Item icon={PencilLine} label="Rename" />
      <Item icon={Copy} label="Duplicate" shortcut="⌘D" />
      <Item icon={Download} label="Download" />

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
                onAction(`Tagged ${t.label}`)
                onClose()
              }}
              className="size-3.5 rounded-full ring-1 ring-inset ring-black/10 transition-transform hover:scale-125"
              style={{ background: t.color }}
            />
          ))}
        </div>
      </div>

      <Separator />

      <Item icon={Trash2} label="Move to Trash" danger shortcut="⌘⌫" />
    </div>
  )
}

function Separator() {
  return <div className="my-1 h-px bg-border/70" />
}
