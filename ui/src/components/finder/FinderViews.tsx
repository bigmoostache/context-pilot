import { ChevronRight } from "lucide-react"
import type { FinderNode, FinderSortKey } from "@/lib/types"
import { fmtBytes, sortNodes } from "@/lib/finderFs"
import { kindMeta, kindTint } from "./kind"
import { cn } from "@/lib/utils"

interface CommonProps {
  selected: Set<string>
  onClick: (node: FinderNode, additive: boolean) => void
  onOpen: (node: FinderNode) => void
}

// ── Grid (icon) view ──────────────────────────────────────────────
export function GridView({
  nodes,
  ...h
}: CommonProps & { nodes: FinderNode[] }) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(108px,1fr))] gap-2 p-4">
      {nodes.map((n) => {
        const M = kindMeta[n.kind]
        const sel = h.selected.has(n.path)
        return (
          <button
            key={n.path}
            onClick={(e) => h.onClick(n, e.metaKey || e.ctrlKey)}
            onDoubleClick={() => h.onOpen(n)}
            className={cn(
              "group flex flex-col items-center gap-2 rounded-xl border p-3 text-center transition-all",
              sel
                ? "border-[var(--signal)]/60 bg-[var(--signal)]/8 card-shadow"
                : "border-transparent hover:border-border hover:bg-muted/50",
            )}
          >
            <span
              className="flex size-14 items-center justify-center rounded-xl transition-transform group-hover:scale-105"
              style={{ background: kindTint(n.kind, sel ? 22 : 14), color: M.accent }}
            >
              <M.icon className="size-7" />
            </span>
            <span className="line-clamp-2 w-full text-[11.5px] font-medium leading-tight text-foreground/85">
              {n.name}
            </span>
            <span className="text-[10px] tabular-nums text-muted-foreground/60">
              {n.kind === "folder" ? `${n.children?.length ?? 0} items` : fmtBytes(n.size)}
            </span>
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
}: CommonProps & {
  nodes: FinderNode[]
  sortKey: FinderSortKey
  asc: boolean
  onSort: (k: FinderSortKey) => void
}) {
  return (
    <div className="px-2 py-1.5">
      <div className="grid grid-cols-[1fr_120px_90px_120px] gap-2 border-b border-border px-2.5 py-1.5 text-[10.5px] font-medium uppercase tracking-wide text-muted-foreground/70">
        <Head label="Name" k="name" cur={sortKey} asc={asc} onSort={onSort} />
        <Head label="Kind" k="kind" cur={sortKey} asc={asc} onSort={onSort} />
        <Head label="Size" k="size" cur={sortKey} asc={asc} onSort={onSort} />
        <Head label="Modified" k="modified" cur={sortKey} asc={asc} onSort={onSort} />
      </div>
      {nodes.map((n) => {
        const M = kindMeta[n.kind]
        const sel = h.selected.has(n.path)
        return (
          <button
            key={n.path}
            onClick={(e) => h.onClick(n, e.metaKey || e.ctrlKey)}
            onDoubleClick={() => h.onOpen(n)}
            className={cn(
              "grid w-full grid-cols-[1fr_120px_90px_120px] items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] transition-colors",
              sel ? "bg-[var(--signal)]/10 text-foreground" : "hover:bg-muted/50 text-foreground/80",
            )}
          >
            <span className="flex min-w-0 items-center gap-2">
              <M.icon className="size-4 shrink-0" style={{ color: M.accent }} />
              <span className="truncate font-medium">{n.name}</span>
            </span>
            <span className="truncate text-muted-foreground">{M.label}</span>
            <span className="tabular-nums text-muted-foreground">
              {n.kind === "folder" ? "—" : fmtBytes(n.size)}
            </span>
            <span className="truncate text-muted-foreground">{n.modified}</span>
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
  selected,
  onClick,
  onOpen,
}: {
  /** Each pane = the children of one folder along the open path. */
  panes: { path: string; nodes: FinderNode[] }[]
  /** Path segment currently chosen at each level (drives highlight). */
  activePath: Set<string>
  selected: Set<string>
  onClick: (node: FinderNode, additive: boolean) => void
  onOpen: (node: FinderNode) => void
}) {
  return (
    <div className="flex h-full min-w-0 overflow-x-auto">
      {panes.map((pane) => (
        <div
          key={pane.path}
          className="flex w-[210px] shrink-0 flex-col overflow-y-auto border-r border-border py-1"
        >
          {sortNodes(pane.nodes, "name", true).map((n) => {
            const M = kindMeta[n.kind]
            const onTrail = activePath.has(n.path)
            const sel = selected.has(n.path)
            return (
              <button
                key={n.path}
                onClick={(e) => {
                  h(onClick, n, e.metaKey || e.ctrlKey)
                  if (n.kind === "folder") onOpen(n)
                }}
                className={cn(
                  "mx-1 flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors",
                  onTrail || sel
                    ? "bg-[var(--signal)]/12 text-foreground"
                    : "text-foreground/80 hover:bg-muted/50",
                )}
              >
                <M.icon className="size-4 shrink-0" style={{ color: M.accent }} />
                <span className="min-w-0 flex-1 truncate font-medium">{n.name}</span>
                {n.kind === "folder" && (
                  <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/50" />
                )}
              </button>
            )
          })}
        </div>
      ))}
    </div>
  )
}

/** Tiny indirection so the columns click-then-maybe-open reads cleanly. */
function h(fn: (n: FinderNode, a: boolean) => void, n: FinderNode, a: boolean) {
  fn(n, a)
}
