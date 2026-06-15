import type { MouseEvent as ReactMouseEvent } from "react"
import { ChevronRight } from "lucide-react"
import type { FinderNode, FinderSortKey, FinderTag } from "@/lib/types"
import { childCounts, fmtBytes, sortNodes } from "@/lib/finderFs"
import { kindGradient, kindMeta, kindTint, TAG_META } from "./kind"
import { cn } from "@/lib/utils"

export interface ViewHandlers {
  selected: Set<string>
  focusPath: string | null
  onClick: (node: FinderNode, mods: { additive: boolean; range: boolean }) => void
  onOpen: (node: FinderNode) => void
  onContext: (e: ReactMouseEvent, node: FinderNode) => void
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

// ── Grid (icon) view ──────────────────────────────────────────────
export function GridView({
  nodes,
  iconSize,
  ...h
}: ViewHandlers & { nodes: FinderNode[]; iconSize: number }) {
  const cell = Math.round(iconSize * 1.55)
  return (
    <div
      className="grid gap-1 p-4"
      style={{ gridTemplateColumns: `repeat(auto-fill,minmax(${cell}px,1fr))` }}
    >
      {nodes.map((n, i) => {
        const M = kindMeta[n.kind]
        const sel = h.selected.has(n.path)
        const focus = h.focusPath === n.path
        return (
          <button
            key={n.path}
            title={n.name}
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
            )}
          >
            <span
              className="flex items-center justify-center rounded-2xl border border-border/40 transition-transform duration-150 group-hover:scale-[1.06] group-active:scale-95"
              style={{
                width: iconSize,
                height: iconSize,
                background: kindGradient(n.kind),
                color: M.accent,
              }}
            >
              <M.icon style={{ width: iconSize * 0.5, height: iconSize * 0.5 }} />
            </span>
            <span className="line-clamp-2 w-full px-0.5 text-[11.5px] font-medium leading-tight text-foreground/85">
              {n.name}
            </span>
            <span className="flex items-center gap-1 text-[10px] tabular-nums text-muted-foreground/60">
              {n.kind === "folder" ? `${n.children?.length ?? 0} items` : fmtBytes(n.size)}
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
        return (
          <button
            key={n.path}
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
            )}
          >
            <span className="flex min-w-0 items-center gap-2">
              <M.icon className="size-4 shrink-0" style={{ color: M.accent }} />
              <span className="truncate font-medium">{n.name}</span>
              <TagDots tags={n.tags} className="shrink-0" />
            </span>
            <span className="truncate text-muted-foreground">{M.label}</span>
            <span className="tabular-nums text-muted-foreground">
              {n.kind === "folder" ? "—" : fmtBytes(n.size)}
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
export function ColumnsView({
  panes,
  activePath,
  previewNode,
  ...h
}: ViewHandlers & {
  panes: { path: string; nodes: FinderNode[] }[]
  activePath: Set<string>
  previewNode: FinderNode | null
}) {
  const showPreviewPane = previewNode && previewNode.kind !== "folder"
  return (
    <div className="flex h-full min-w-0 overflow-x-auto">
      {panes.map((pane) => (
        <div
          key={pane.path}
          className="flex w-[218px] shrink-0 flex-col overflow-y-auto border-r border-border py-1"
        >
          {sortNodes(pane.nodes, "name", true).map((n) => {
            const M = kindMeta[n.kind]
            const onTrail = activePath.has(n.path)
            const sel = h.selected.has(n.path)
            return (
              <button
                key={n.path}
                onClick={(e) => {
                  h.onClick(n, mods(e))
                  if (n.kind === "folder") h.onOpen(n)
                }}
                onDoubleClick={() => h.onOpen(n)}
                onContextMenu={(e) => h.onContext(e, n)}
                className={cn(
                  "mx-1 flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors",
                  onTrail || sel
                    ? "bg-[var(--signal)]/14 text-foreground"
                    : "text-foreground/80 hover:bg-muted/45",
                )}
              >
                <M.icon className="size-4 shrink-0" style={{ color: M.accent }} />
                <span className="min-w-0 flex-1 truncate font-medium">{n.name}</span>
                <TagDots tags={n.tags} />
                {n.kind === "folder" && (
                  <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/50" />
                )}
              </button>
            )
          })}
        </div>
      ))}

      {showPreviewPane && previewNode && (
        <div className="flex w-[230px] shrink-0 flex-col items-center gap-3 px-5 py-7 text-center">
          <span
            className="flex size-20 items-center justify-center rounded-2xl border border-border/50"
            style={{ background: kindGradient(previewNode.kind), color: kindMeta[previewNode.kind].accent }}
          >
            {(() => {
              const I = kindMeta[previewNode.kind].icon
              return <I className="size-10" />
            })()}
          </span>
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
          const M = kindMeta[n.kind]
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
              <span
                className="flex size-12 items-center justify-center rounded-lg border border-border/40"
                style={
                  n.image
                    ? { background: n.image.gradient }
                    : { background: kindGradient(n.kind), color: M.accent }
                }
              >
                {!n.image && <M.icon className="size-6" />}
              </span>
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
  const counts = childCounts(node)
  return (
    <div className="ql-pop flex max-h-full max-w-[560px] flex-col items-center gap-4">
      {node.image ? (
        <div
          className="aspect-video w-[460px] max-w-full rounded-xl border border-border card-shadow"
          style={{ background: node.image.gradient }}
        />
      ) : (
        <span
          className="flex size-36 items-center justify-center rounded-3xl border border-border/50 card-shadow"
          style={{ background: kindGradient(node.kind), color: M.accent }}
        >
          <M.icon className="size-20" />
        </span>
      )}
      <div className="flex flex-col items-center gap-1.5 text-center">
        <span className="text-[17px] font-semibold tracking-tight text-foreground">{node.name}</span>
        <TagDots tags={node.tags} />
        <span className="text-[12px] tabular-nums text-muted-foreground">
          {node.kind === "folder"
            ? `${counts.folders} folders · ${counts.files} files`
            : `${M.label} · ${fmtBytes(node.size)}`}
          {node.media ? ` · ${node.media.duration}` : ""}
          {node.image ? ` · ${node.image.w}×${node.image.h}` : ""}
        </span>
      </div>
    </div>
  )
}

export { kindTint }
