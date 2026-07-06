import { useState } from "react"
import type { FinderNode, FinderSortKey } from "@/lib/types"
import { fmtBytes } from "@/lib/support/finderFs"
import { extOf, kindMeta } from "../support/kind"
import { FileIcon } from "../support/macIcons"
import { InfoBadge } from "../support/InfoBadge"
import { cn } from "@/lib/utils"
import {
  folderDropProps,
  mods,
  startItemDrag,
  itemCount,
  type ViewHandlers,
} from "./helpers"
import { RenameInput, TagDots } from "./shared"

// Column view is a sibling component; re-exported so `views/FinderViews` stays
// the single import surface for the four Finder views (Finder.tsx). Non-component
// shared symbols (MIME consts, ViewHandlers, TagDots) are imported directly from
// `./helpers` / `./shared` by their consumers — re-exporting them here would
// break Fast Refresh (a component module must export components only).
export { ColumnsView } from "./ColumnsView"

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
              "finder-pop group relative flex flex-col items-center gap-1.5 rounded-xl border p-2.5 text-center transition-[background,border,transform] duration-150",
              sel
                ? "border-[var(--signal)]/55 bg-[var(--signal)]/10 card-shadow"
                : "border-transparent hover:border-border hover:bg-muted/45",
              focus && !sel && "ring-2 ring-[var(--signal)]/45",
              dropOver && "border-[var(--signal)] bg-[var(--signal)]/15 ring-2 ring-[var(--signal)]/60",
            )}
          >
            {h.descriptions?.[n.path] && (
              <span className="absolute right-1 top-1 z-[1]">
                <InfoBadge description={h.descriptions[n.path]} />
              </span>
            )}
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
              {h.descriptions?.[n.path] && <InfoBadge description={h.descriptions[n.path]} />}
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


