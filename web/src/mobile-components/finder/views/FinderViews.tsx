import type { FinderNode, FinderSortKey } from "@/lib/types"
import { fmtBytes } from "@/lib/support/finderFs"
import { extOf, kindMeta } from "../support/kind"
import { FileIcon } from "../support/macIcons"
import { InfoBadge } from "../support/InfoBadge"
import { cn } from "@/lib/utils"
import { mods, startItemDrag, itemCount, type ViewHandlers } from "./helpers"
import { RenameInput, TagDots } from "./shared"

// Column view is a sibling component; re-exported so `views/FinderViews` stays
// the single import surface for the four Finder views (Finder/body). Same
// Fast-Refresh constraint as desktop — component modules export components only.
export { ColumnsView } from "./ColumnsView"

/**
 * The mobile tap contract, shared by every view. Touch has no hover-preview and
 * no double-click: a single tap on a **folder drills in** (onOpen), a single tap
 * on a **file selects it** (onClick, which also drives the Quick Look preview),
 * and a **long-press** opens the context action sheet (onContext). This replaces
 * the desktop click=select / double-click=open / right-click=menu split. Drag
 * (startItemDrag) is kept for internal moves; the marquee band is dropped (no
 * rubber-band select on touch).
 */
function tapProps(n: FinderNode, h: ViewHandlers) {
  return {
    onClick: (e: React.MouseEvent) => {
      if (n.kind === "folder") h.onOpen(n)
      else h.onClick(n, mods(e))
    },
    onContextMenu: (e: React.MouseEvent) => h.onContext(e, n),
  }
}

// ── Grid (icon) view ──────────────────────────────────────────────
export function GridView({
  nodes,
  iconSize,
  ...h
}: ViewHandlers & { nodes: FinderNode[]; iconSize: number }) {
  // Larger minimum cell so every tile is a comfortable tap target (min ~92px).
  const cell = Math.max(92, Math.round(iconSize * 1.7))
  return (
    <div
      className="grid gap-2 p-3"
      style={{ gridTemplateColumns: `repeat(auto-fill,minmax(${cell}px,1fr))` }}
    >
      {nodes.map((n, i) => {
        const sel = h.selected.has(n.path)
        return (
          <button
            key={n.path}
            title={n.name}
            data-finder-item=""
            data-path={n.path}
            draggable
            onDragStart={(e) => startItemDrag(e, n, h.selected)}
            {...tapProps(n, h)}
            style={{ animationDelay: `${Math.min(i, 18) * 22}ms` }}
            className={cn(
              "finder-pop group relative flex flex-col items-center gap-1.5 rounded-xl border p-3 text-center transition-[background,border,transform] duration-150",
              sel
                ? "card-shadow border-(--signal)/55 bg-(--signal)/10"
                : "border-transparent active:border-border active:bg-muted/45",
            )}
          >
            {h.descriptions?.[n.path] && (
              <span className="absolute top-1 right-1 z-1">
                <InfoBadge description={h.descriptions[n.path]} />
              </span>
            )}
            <FileIcon
              kind={n.kind}
              ext={extOf(n.name)}
              size={iconSize}
              className="transition-transform duration-150 group-active:scale-95"
            />
            {h.renamingPath === n.path && h.onRenameCommit && h.onRenameCancel ? (
              <span
                className="flex w-full px-0.5"
                role="presentation"
                onClick={(e) => e.stopPropagation()}
              >
                <RenameInput
                  node={n}
                  onCommit={(name) => h.onRenameCommit?.(n, name)}
                  onCancel={() => h.onRenameCancel?.()}
                />
              </span>
            ) : (
              <span className="line-clamp-2 w-full px-0.5 text-[12px] leading-tight font-medium text-foreground/85">
                {n.name}
              </span>
            )}
            <span className="flex items-center gap-1 text-[10.5px] text-muted-foreground/60 tabular-nums">
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
// On a phone the desktop 4-column grid (name/kind/size/modified) doesn't fit;
// collapse to name + a single secondary meta line, taller rows for tapping.
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
  return (
    <div className="px-2 py-1.5">
      {/* single sort control strip (the 4-col header row can't fit) */}
      <div className="flex items-center gap-2 px-2.5 py-1.5 text-[10.5px] font-medium tracking-wide text-muted-foreground/70 uppercase">
        <SortPill label="Name" k="name" cur={sortKey} asc={asc} onSort={onSort} />
        <SortPill label="Size" k="size" cur={sortKey} asc={asc} onSort={onSort} />
        <SortPill label="Modified" k="modified" cur={sortKey} asc={asc} onSort={onSort} />
      </div>
      {nodes.map((n) => {
        const M = kindMeta[n.kind]
        const sel = h.selected.has(n.path)
        return (
          <button
            key={n.path}
            data-finder-item=""
            data-path={n.path}
            draggable
            onDragStart={(e) => startItemDrag(e, n, h.selected)}
            {...tapProps(n, h)}
            className={cn(
              "flex w-full items-center gap-3 rounded-md p-2.5 text-left text-[13.5px] transition-colors",
              sel ? "bg-(--signal)/12 text-foreground" : "text-foreground/80 active:bg-muted/45",
            )}
          >
            <FileIcon kind={n.kind} ext={extOf(n.name)} size={22} className="shrink-0" />
            <span className="flex min-w-0 flex-1 flex-col">
              {h.renamingPath === n.path && h.onRenameCommit && h.onRenameCancel ? (
                <RenameInput
                  node={n}
                  onCommit={(name) => h.onRenameCommit?.(n, name)}
                  onCancel={() => h.onRenameCancel?.()}
                />
              ) : (
                <span className="flex items-center gap-1.5">
                  <span className="truncate font-medium">{n.name}</span>
                  <TagDots tags={n.tags} className="shrink-0" />
                  {h.descriptions?.[n.path] && <InfoBadge description={h.descriptions[n.path]} />}
                </span>
              )}
              <span className="truncate text-[11px] text-muted-foreground">
                {M.label} · {n.kind === "folder" ? itemCount(n) : fmtBytes(n.size)} · {n.modified}
              </span>
            </span>
          </button>
        )
      })}
    </div>
  )
}

/** Segmented sort pill for the mobile list header (replaces the tap-column
 *  headers — a phone has no room for a 4-column header grid). */
function SortPill({
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
  const on = cur === k
  return (
    <button
      onClick={() => onSort(k)}
      className={cn(
        "flex items-center gap-0.5 rounded-md px-2 py-1 transition-colors",
        on ? "bg-(--signal)/15 text-(--signal)" : "active:bg-muted/50",
      )}
    >
      {label}
      {on && <span>{asc ? "▲" : "▼"}</span>}
    </button>
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
      <div className="flex min-h-0 flex-1 items-center justify-center p-6">
        {node ? (
          <Hero node={node} />
        ) : (
          <span className="text-[13px] text-muted-foreground">Empty</span>
        )}
      </div>
      <div className="no-scrollbar flex shrink-0 items-end gap-2 overflow-x-auto border-t border-border bg-surface p-3">
        {nodes.map((n) => {
          const active = node?.path === n.path
          return (
            <button
              key={n.path}
              title={n.name}
              {...tapProps(n, h)}
              className={cn(
                "flex shrink-0 flex-col items-center gap-1 rounded-lg p-1.5 transition-all",
                active ? "bg-(--signal)/14 ring-1 ring-(--signal)/50" : "active:bg-muted/50",
              )}
            >
              {n.image ? (
                <span
                  className="flex size-14 items-center justify-center rounded-lg border border-border/40"
                  style={{ background: n.image.gradient }}
                />
              ) : (
                <FileIcon kind={n.kind} ext={extOf(n.name)} size={52} />
              )}
              <span className="max-w-[76px] truncate text-[10px] text-muted-foreground">
                {n.name}
              </span>
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
    <div className="ql-pop flex max-h-full max-w-full flex-col items-center gap-4">
      {node.image ? (
        <div
          className="card-shadow aspect-video w-full max-w-[460px] rounded-xl border border-border"
          style={{ background: node.image.gradient }}
        />
      ) : (
        <FileIcon kind={node.kind} ext={extOf(node.name)} size={120} className="card-shadow" />
      )}
      <div className="flex flex-col items-center gap-1.5 text-center">
        <span className="text-[17px] font-semibold tracking-tight text-foreground">
          {node.name}
        </span>
        <TagDots tags={node.tags} />
        <span className="text-[12px] text-muted-foreground tabular-nums">
          {node.kind === "folder" ? itemCount(node) : `${M.label} · ${fmtBytes(node.size)}`}
          {node.media ? ` · ${node.media.duration}` : ""}
          {node.image ? ` · ${node.image.w}×${node.image.h}` : ""}
        </span>
      </div>
    </div>
  )
}
