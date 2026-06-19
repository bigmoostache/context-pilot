import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from "react"
import { useEffect, useRef, useState } from "react"
import { ChevronRight } from "lucide-react"
import type { FinderNode, FinderSortKey, FinderTag } from "@/lib/types"
import { fmtBytes, sortNodes } from "@/lib/finderFs"
import { useFs } from "@/lib/live"
import { extOf, kindMeta, kindTint, TAG_META } from "./kind"
import { FileIcon } from "./macIcons"
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
function startItemDrag(e: ReactDragEvent, n: FinderNode, selected: Set<string>) {
  const paths = selected.has(n.path) && selected.size > 0 ? [...selected] : [n.path]
  e.dataTransfer.setData(MOVE_MIME, JSON.stringify({ paths }))
  e.dataTransfer.effectAllowed = "move"
  // A single folder can ALSO be pinned by dropping on the sidebar.
  if (n.kind === "folder" && paths.length === 1) {
    e.dataTransfer.setData(FOLDER_DRAG_MIME, JSON.stringify({ name: n.name, path: n.path }))
  }
}

/** Read the internal move payload off a drop event, if present. */
function readMovePayload(e: ReactDragEvent): string[] | null {
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
function isMoveDrag(e: ReactDragEvent): boolean {
  return e.dataTransfer.types.includes(MOVE_MIME)
}

/**
 * Drop-target handlers for a FOLDER row (any view). When an internal move-drag
 * hovers, the folder highlights (`setOver(path)`) and accepts `move`; on drop it
 * routes the dragged paths to `onMove(paths, folder)` — unless the folder is
 * itself one of the dragged items (can't drop onto self). A no-op when `onMove`
 * is absent or the drag isn't an internal move.
 */
function folderDropProps(
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
function relOf(agentFolder: string, abs: string): string {
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
}

/**
 * Inline rename field — a macOS-style editable name cell. Mounts focused with the
 * basename (sans extension) pre-selected, commits on Enter or blur, cancels on
 * Esc. Keydown is stopped from bubbling so the Finder surface's own key handler
 * (arrows / type-ahead / Enter-to-rename) never fires while the user types.
 */
function RenameInput({
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

const mods = (e: ReactMouseEvent) => ({
  additive: e.metaKey || e.ctrlKey,
  range: e.shiftKey,
})

/**
 * Human "N items" label for a folder. Uses the backend-supplied direct child
 * `count` (live data); falls back to a populated `children` array (mock realm)
 * so both the live app and the maquette render a real number, never "0 items"
 * for a non-empty folder.
 */
function itemCount(n: FinderNode): string {
  const c = n.count ?? n.children?.length ?? 0
  return `${c} ${c === 1 ? "item" : "items"}`
}

// ── Grid (icon) view ──────────────────────────────────────────────
export function GridView({
  nodes,
  iconSize,
  ...h
}: ViewHandlers & { nodes: FinderNode[]; iconSize: number }) {
  const cell = Math.round(iconSize * 1.55)
  const [dragOver, setDragOver] = useState<string | null>(null)
  return (
    <div
      className="grid gap-1 p-4"
      style={{ gridTemplateColumns: `repeat(auto-fill,minmax(${cell}px,1fr))` }}
    >
      {nodes.map((n, i) => {
        const sel = h.selected.has(n.path)
        const focus = h.focusPath === n.path
        const dropOver = dragOver === n.path
        return (
          <button
            key={n.path}
            title={n.name}
            data-finder-item=""
            data-path={n.path}
            draggable
            onDragStart={(e) => startItemDrag(e, n, h.selected)}
            {...folderDropProps(n, dropOver, setDragOver, h.onMove)}
            onClick={(e) => h.onClick(n, mods(e))}
            onDoubleClick={() => h.onOpen(n)}
            onContextMenu={(e) => h.onContext(e, n)}
            style={{ animationDelay: `${Math.min(i, 18) * 22}ms` }}
            className={cn(
              "finder-pop group flex flex-col items-center gap-1.5 rounded-xl border p-2.5 text-center transition-[background,border,transform] duration-150",
              sel
                ? "border-[var(--signal)]/55 bg-[var(--signal)]/10 card-shadow"
                : "border-transparent hover:border-border hover:bg-muted/45",
              focus && !sel && "ring-2 ring-[var(--signal)]/45",
              dropOver && "border-[var(--signal)] bg-[var(--signal)]/15 ring-2 ring-[var(--signal)]/60",
            )}
          >
            <FileIcon
              kind={n.kind}
              ext={extOf(n.name)}
              size={iconSize}
              className="transition-transform duration-150 group-hover:scale-[1.06] group-active:scale-95"
            />
            {h.renamingPath === n.path && h.onRenameCommit && h.onRenameCancel ? (
              <span className="flex w-full px-0.5" onClick={(e) => e.stopPropagation()}>
                <RenameInput
                  node={n}
                  onCommit={(name) => h.onRenameCommit?.(n, name)}
                  onCancel={() => h.onRenameCancel?.()}
                />
              </span>
            ) : (
              <span className="line-clamp-2 w-full px-0.5 text-[11.5px] font-medium leading-tight text-foreground/85">
                {n.name}
              </span>
            )}
            <span className="flex items-center gap-1 text-[10px] tabular-nums text-muted-foreground/60">
              {n.kind === "folder" ? itemCount(n) : fmtBytes(n.size)}
            </span>
            <TagDots tags={n.tags} />
          </button>
        )
      })}
    </div>
  )
}

// ── List (details) view ───────────────────────────────────────────
export function ListView({
  nodes,
  sortKey,
  asc,
  onSort,
  ...h
}: ViewHandlers & {
  nodes: FinderNode[]
  sortKey: FinderSortKey
  asc: boolean
  onSort: (k: FinderSortKey) => void
}) {
  const [dragOver, setDragOver] = useState<string | null>(null)
  return (
    <div className="px-2 py-1.5">
      <div className="sticky top-0 z-[1] grid grid-cols-[1fr_120px_92px_120px] gap-2 bg-background/90 px-2.5 py-1.5 text-[10.5px] font-medium uppercase tracking-wide text-muted-foreground/70 backdrop-blur">
        <Head label="Name" k="name" cur={sortKey} asc={asc} onSort={onSort} />
        <Head label="Kind" k="kind" cur={sortKey} asc={asc} onSort={onSort} />
        <Head label="Size" k="size" cur={sortKey} asc={asc} onSort={onSort} />
        <Head label="Modified" k="modified" cur={sortKey} asc={asc} onSort={onSort} />
      </div>
      {nodes.map((n, i) => {
        const M = kindMeta[n.kind]
        const sel = h.selected.has(n.path)
        const focus = h.focusPath === n.path
        const dropOver = dragOver === n.path
        return (
          <button
            key={n.path}
            data-finder-item=""
            data-path={n.path}
            draggable
            onDragStart={(e) => startItemDrag(e, n, h.selected)}
            {...folderDropProps(n, dropOver, setDragOver, h.onMove)}
            onClick={(e) => h.onClick(n, mods(e))}
            onDoubleClick={() => h.onOpen(n)}
            onContextMenu={(e) => h.onContext(e, n)}
            style={{ animationDelay: `${Math.min(i, 22) * 14}ms` }}
            className={cn(
              "finder-pop grid w-full grid-cols-[1fr_120px_92px_120px] items-center gap-2 rounded-md px-2.5 py-[5px] text-left text-[12px] transition-colors",
              sel
                ? "bg-[var(--signal)]/12 text-foreground"
                : cn("text-foreground/80 hover:bg-muted/45", i % 2 === 1 && "bg-muted/20"),
              focus && !sel && "ring-1 ring-inset ring-[var(--signal)]/50",
              dropOver && "bg-[var(--signal)]/18 ring-1 ring-inset ring-[var(--signal)]/70",
            )}
          >
            <span className="flex min-w-0 items-center gap-2">
              <FileIcon kind={n.kind} ext={extOf(n.name)} size={17} className="shrink-0" />
              {h.renamingPath === n.path && h.onRenameCommit && h.onRenameCancel ? (
                <RenameInput
                  node={n}
                  onCommit={(name) => h.onRenameCommit?.(n, name)}
                  onCancel={() => h.onRenameCancel?.()}
                />
              ) : (
                <span className="truncate font-medium">{n.name}</span>
              )}
              <TagDots tags={n.tags} className="shrink-0" />
            </span>
            <span className="truncate text-muted-foreground">{M.label}</span>
            <span className="tabular-nums text-muted-foreground">
              {n.kind === "folder" ? itemCount(n) : fmtBytes(n.size)}
            </span>
            <span className="truncate text-muted-foreground" title={n.created ? `Created ${n.created}` : undefined}>
              {n.modified}
            </span>
          </button>
        )
      })}
    </div>
  )
}

function Head({
  label,
  k,
  cur,
  asc,
  onSort,
}: {
  label: string
  k: FinderSortKey
  cur: FinderSortKey
  asc: boolean
  onSort: (k: FinderSortKey) => void
}) {
  return (
    <button
      onClick={() => onSort(k)}
      className="flex items-center gap-1 text-left transition-colors hover:text-foreground"
    >
      {label}
      {cur === k && <span className="text-[var(--signal)]">{asc ? "▲" : "▼"}</span>}
    </button>
  )
}

// ── Columns (Miller) view ─────────────────────────────────────────
/**
 * Miller-columns browser over LIVE data. One column per ancestor in the path
 * chain (root → cwd): each column lists that folder's children, with the child
 * that leads to the next column highlighted, so the whole traversed hierarchy
 * is visible at once (the point of column view) — not just the current folder.
 *
 * Every ancestor column fetches its own children via `useFs`; the deepest
 * column reuses the already-fetched + filtered + sorted `currentNodes`. A
 * trailing pane previews the selected file. Clicking a folder in any column
 * navigates into it (truncating the chain past that point).
 */
export function ColumnsView({
  agentId,
  agentFolder,
  chain,
  currentNodes,
  previewNode,
  onNavigate,
  ...h
}: ViewHandlers & {
  agentId: string
  agentFolder: string
  /** absolute paths from realm root down to the current working directory */
  chain: string[]
  /** the current dir's already-filtered+sorted nodes (deepest column) */
  currentNodes: FinderNode[]
  previewNode: FinderNode | null
  onNavigate: (path: string) => void
}) {
  const showPreviewPane = previewNode && previewNode.kind !== "folder"
  return (
    <div className="flex h-full min-w-0 overflow-x-auto">
      {chain.map((path, i) => (
        <MillerColumn
          key={path}
          agentId={agentId}
          agentFolder={agentFolder}
          path={path}
          nextPath={chain[i + 1]}
          nodes={i === chain.length - 1 ? currentNodes : undefined}
          onNavigate={onNavigate}
          {...h}
        />
      ))}

      {showPreviewPane && previewNode && (
        <div className="flex w-[230px] shrink-0 flex-col items-center gap-3 px-5 py-7 text-center">
          <FileIcon kind={previewNode.kind} ext={extOf(previewNode.name)} size={84} />
          <span className="text-[13px] font-semibold text-foreground/90">{previewNode.name}</span>
          <TagDots tags={previewNode.tags} />
          <dl className="mt-1 flex w-full flex-col gap-1 text-[11px]">
            <PaneRow k="Kind" v={kindMeta[previewNode.kind].label} />
            <PaneRow k="Size" v={fmtBytes(previewNode.size)} />
            <PaneRow k="Modified" v={previewNode.modified} />
          </dl>
        </div>
      )}
    </div>
  )
}

/**
 * One Miller column = the listing of a single folder in the path chain. Ancestor
 * columns fetch their own children live; the deepest column receives the current
 * dir's nodes directly (so search/sort already applied). The row whose path is
 * `nextPath` (the traversed child) is highlighted as "on trail".
 */
function MillerColumn({
  agentId,
  agentFolder,
  path,
  nextPath,
  nodes: provided,
  onNavigate,
  ...h
}: ViewHandlers & {
  agentId: string
  agentFolder: string
  path: string
  nextPath?: string
  /** present for the deepest column (current dir) — skips the fetch */
  nodes?: FinderNode[]
  onNavigate: (path: string) => void
}) {
  // Hook is always called (rules of hooks); its data is ignored when `provided`.
  const { data } = useFs(agentId, relOf(agentFolder, path))
  const nodes = sortNodes(provided ?? data ?? [], "name", true)
  const [dragOver, setDragOver] = useState<string | null>(null)
  return (
    <div className="flex w-[218px] shrink-0 flex-col overflow-y-auto border-r border-border py-1">
      {nodes.map((n) => {
        const onTrail = n.path === nextPath
        const sel = h.selected.has(n.path)
        const dropOver = dragOver === n.path
        return (
          <button
            key={n.path}
            draggable
            onDragStart={(e) => startItemDrag(e, n, h.selected)}
            {...folderDropProps(n, dropOver, setDragOver, h.onMove)}
            onClick={(e) => {
              h.onClick(n, mods(e))
              if (n.kind === "folder") onNavigate(n.path)
            }}
            onDoubleClick={() => h.onOpen(n)}
            onContextMenu={(e) => h.onContext(e, n)}
            className={cn(
              // `select-none` is load-bearing here: unlike grid/list (whose
              // <main> carries select-none via the marquee), the columns view
              // has none, so a native press-drag on a row's text would start a
              // TEXT selection instead of an element drag — the drag never
              // fires and the move silently fails (T287). Suppressing text
              // selection lets `draggable` initiate the element drag reliably.
              "mx-1 flex select-none items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors",
              onTrail || sel
                ? "bg-[var(--signal)]/14 text-foreground"
                : "text-foreground/80 hover:bg-muted/45",
              dropOver && "bg-[var(--signal)]/20 ring-1 ring-inset ring-[var(--signal)]/70",
            )}
          >
            <FileIcon kind={n.kind} ext={extOf(n.name)} size={17} className="shrink-0" />
            {h.renamingPath === n.path && h.onRenameCommit && h.onRenameCancel ? (
              <RenameInput
                node={n}
                onCommit={(name) => h.onRenameCommit?.(n, name)}
                onCancel={() => h.onRenameCancel?.()}
              />
            ) : (
              <span className="min-w-0 flex-1 truncate font-medium">{n.name}</span>
            )}
            <TagDots tags={n.tags} />
            {n.kind === "folder" && (
              <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/50" />
            )}
          </button>
        )
      })}
    </div>
  )
}

function PaneRow({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex items-baseline justify-between gap-2">
      <dt className="text-muted-foreground">{k}</dt>
      <dd className="truncate text-foreground/80">{v}</dd>
    </div>
  )
}

// ── Gallery view (hero + filmstrip) ───────────────────────────────
export function GalleryView({
  nodes,
  hero,
  ...h
}: ViewHandlers & { nodes: FinderNode[]; hero: FinderNode | null }) {
  const node = hero ?? nodes[0] ?? null
  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* hero */}
      <div className="flex min-h-0 flex-1 items-center justify-center p-8">
        {node ? <Hero node={node} /> : <span className="text-[13px] text-muted-foreground">Empty</span>}
      </div>

      {/* filmstrip */}
      <div className="flex shrink-0 items-end gap-2 overflow-x-auto border-t border-border bg-surface px-4 py-3">
        {nodes.map((n) => {
          const active = node?.path === n.path
          return (
            <button
              key={n.path}
              title={n.name}
              onClick={(e) => h.onClick(n, mods(e))}
              onDoubleClick={() => h.onOpen(n)}
              onContextMenu={(e) => h.onContext(e, n)}
              className={cn(
                "flex shrink-0 flex-col items-center gap-1 rounded-lg p-1.5 transition-all",
                active ? "bg-[var(--signal)]/14 ring-1 ring-[var(--signal)]/50" : "hover:bg-muted/50",
              )}
            >
              {n.image ? (
                <span
                  className="flex size-12 items-center justify-center rounded-lg border border-border/40"
                  style={{ background: n.image.gradient }}
                />
              ) : (
                <FileIcon kind={n.kind} ext={extOf(n.name)} size={48} />
              )}
              <span className="max-w-[68px] truncate text-[10px] text-muted-foreground">{n.name}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

function Hero({ node }: { node: FinderNode }) {
  const M = kindMeta[node.kind]
  return (
    <div className="ql-pop flex max-h-full max-w-[560px] flex-col items-center gap-4">
      {node.image ? (
        <div
          className="aspect-video w-[460px] max-w-full rounded-xl border border-border card-shadow"
          style={{ background: node.image.gradient }}
        />
      ) : (
        <FileIcon kind={node.kind} ext={extOf(node.name)} size={132} className="card-shadow" />
      )}
      <div className="flex flex-col items-center gap-1.5 text-center">
        <span className="text-[17px] font-semibold tracking-tight text-foreground">{node.name}</span>
        <TagDots tags={node.tags} />
        <span className="text-[12px] tabular-nums text-muted-foreground">
          {node.kind === "folder"
            ? itemCount(node)
            : `${M.label} · ${fmtBytes(node.size)}`}
          {node.media ? ` · ${node.media.duration}` : ""}
          {node.image ? ` · ${node.image.w}×${node.image.h}` : ""}
        </span>
      </div>
    </div>
  )
}

export { kindTint }
