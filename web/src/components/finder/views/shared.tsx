import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from "react"
import { useEffect, useRef } from "react"
import type { FinderNode, FinderTag } from "@/lib/types"
import { TAG_META } from "../support/kind"
import { cn } from "@/lib/utils"

/** MIME used when dragging a folder out of a view onto the sidebar to pin it. */
export const FOLDER_DRAG_MIME = "application/x-cp-folder"

/** MIME used for INTERNAL item drags (move a selection into a folder). Carries
 *  a JSON `{ paths: string[] }` of the realm-relative entries being dragged. Its
 *  mere presence on a drag tells the surface this is an internal move — NOT an
 *  external OS file drop — so the "Drop to upload" overlay stays hidden. */
export const MOVE_MIME = "application/x-cp-move"

/**
 * Begin an internal move-drag of `n`. If `n` is part of the current multi-select
 * the WHOLE selection travels; otherwise just `n`. A lone folder also carries
 * {@link FOLDER_DRAG_MIME} so sidebar-pinning still works. The payload is the
 * realm-relative paths under {@link MOVE_MIME}.
 */
export function startItemDrag(e: ReactDragEvent, n: FinderNode, selected: Set<string>) {
  const paths = selected.has(n.path) && selected.size > 0 ? [...selected] : [n.path]
  e.dataTransfer.setData(MOVE_MIME, JSON.stringify({ paths }))
  e.dataTransfer.effectAllowed = "move"
  // A single folder can ALSO be pinned by dropping on the sidebar.
  if (n.kind === "folder" && paths.length === 1) {
    e.dataTransfer.setData(FOLDER_DRAG_MIME, JSON.stringify({ name: n.name, path: n.path }))
  }
}

/** Read the internal move payload off a drop event, if present. */
export function readMovePayload(e: ReactDragEvent): string[] | null {
  const raw = e.dataTransfer.getData(MOVE_MIME)
  if (!raw) return null
  try {
    const p = JSON.parse(raw) as { paths?: string[] }
    return Array.isArray(p.paths) && p.paths.length > 0 ? p.paths : null
  } catch {
    return null
  }
}

/** True when a drag carries our internal move payload (vs. an OS file drop). */
export function isMoveDrag(e: ReactDragEvent): boolean {
  return e.dataTransfer.types.includes(MOVE_MIME)
}

/**
 * Drop-target handlers for a FOLDER row (any view). When an internal move-drag
 * hovers, the folder highlights (`setOver(path)`) and accepts `move`; on drop it
 * routes the dragged paths to `onMove(paths, folder)` — unless the folder is
 * itself one of the dragged items (can't drop onto self). A no-op when `onMove`
 * is absent or the drag isn't an internal move.
 */
export function folderDropProps(
  n: FinderNode,
  isOver: boolean,
  setOver: (p: string | null) => void,
  onMove: ViewHandlers["onMove"],
) {
  if (!onMove || n.kind !== "folder") return {}
  return {
    onDragOver: (e: ReactDragEvent) => {
      if (!isMoveDrag(e)) return
      const dragged = readMovePayload(e)
      if (dragged?.includes(n.path)) return // can't drop onto self
      e.preventDefault()
      e.dataTransfer.dropEffect = "move"
      if (!isOver) setOver(n.path)
    },
    onDragLeave: (e: ReactDragEvent) => {
      if (e.currentTarget === e.target) setOver(null)
    },
    onDrop: (e: ReactDragEvent) => {
      if (!isMoveDrag(e)) return
      e.preventDefault()
      e.stopPropagation()
      setOver(null)
      const dragged = readMovePayload(e)
      if (dragged && !dragged.includes(n.path)) onMove(dragged, n)
    },
  }
}

/** Strip the agent-folder prefix to get the backend-relative path for `useFs`. */
export function relOf(agentFolder: string, abs: string): string {
  if (abs === agentFolder) return ""
  if (abs.startsWith(agentFolder + "/")) return abs.slice(agentFolder.length + 1)
  return abs
}

export interface ViewHandlers {
  selected: Set<string>
  focusPath: string | null
  onClick: (node: FinderNode, mods: { additive: boolean; range: boolean }) => void
  onOpen: (node: FinderNode) => void
  onContext: (e: ReactMouseEvent, node: FinderNode) => void
  /** Move the given realm-relative paths into the destination folder (internal
   *  drag-and-drop). Absent in views that don't support drop targets. */
  onMove?: (paths: string[], destFolder: FinderNode) => void
  /** path of the entry currently being inline-renamed (its name cell renders an
   *  editable field instead of a label). Null when nothing is being renamed. */
  renamingPath?: string | null
  /** commit a rename to `newName` (Enter / blur). Trim + no-op handling lives in
   *  the parent; an empty/unchanged name should be treated as a cancel there. */
  onRenameCommit?: (node: FinderNode, newName: string) => void
  /** abandon the in-progress rename (Esc). */
  onRenameCancel?: () => void
  /** realm-relative path → tree-description map (the agent's tree-describe
   *  output). A node whose `path` is a key shows an info badge revealing the
   *  description on hover/click. Absent when the realm has no descriptions. */
  descriptions?: Record<string, string>
}

/**
 * Inline rename field — a macOS-style editable name cell. Mounts focused with the
 * basename (sans extension) pre-selected, commits on Enter or blur, cancels on
 * Esc. Keydown is stopped from bubbling so the Finder surface's own key handler
 * (arrows / type-ahead / Enter-to-rename) never fires while the user types.
 */
export function RenameInput({
  node,
  onCommit,
  onCancel,
}: {
  node: FinderNode
  onCommit: (newName: string) => void
  onCancel: () => void
}) {
  const ref = useRef<HTMLInputElement>(null)
  // Select the basename (everything before the last dot) on mount, like Finder —
  // so a quick retype keeps the extension. A dotfile / extensionless name selects
  // whole.
  useEffect(() => {
    const el = ref.current
    if (!el) return
    el.focus()
    const dot = node.name.lastIndexOf(".")
    el.setSelectionRange(0, dot > 0 ? dot : node.name.length)
  }, [node.name])

  return (
    <input
      ref={ref}
      type="text"
      defaultValue={node.name}
      spellCheck={false}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation()
        if (e.key === "Enter") {
          e.preventDefault()
          onCommit((e.target as HTMLInputElement).value)
        } else if (e.key === "Escape") {
          e.preventDefault()
          onCancel()
        }
      }}
      onBlur={(e) => onCommit(e.target.value)}
      className="min-w-0 flex-1 rounded-[4px] border border-[var(--signal)] bg-background px-1 py-px text-[12px] text-foreground outline-none ring-2 ring-[var(--signal)]/40"
    />
  )
}

/** Colored macOS finder tag dots. */
export function TagDots({ tags, className }: { tags?: FinderTag[]; className?: string }) {
  if (!tags || tags.length === 0) return null
  return (
    <span className={cn("flex items-center gap-0.5", className)}>
      {tags.map((t) => (
        <span
          key={t}
          title={TAG_META[t].label}
          className="size-2 rounded-full ring-1 ring-inset ring-black/10"
          style={{ background: TAG_META[t].color }}
        />
      ))}
    </span>
  )
}

/** Extract click modifier flags (additive = cmd/ctrl, range = shift). */
export const mods = (e: ReactMouseEvent) => ({
  additive: e.metaKey || e.ctrlKey,
  range: e.shiftKey,
})

/**
 * Human "N items" label for a folder. Uses the backend-supplied direct child
 * `count` (live data); falls back to a populated `children` array (mock realm)
 * so both the live app and the maquette render a real number, never "0 items"
 * for a non-empty folder.
 */
export function itemCount(n: FinderNode): string {
  const c = n.count ?? n.children?.length ?? 0
  return `${c} ${c === 1 ? "item" : "items"}`
}
