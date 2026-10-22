import { ArrowDown, ArrowUp, ChevronsUpDown } from "lucide-react"
import { useRef, type CSSProperties, type PointerEvent, type ReactNode } from "react"

import { cn } from "@/lib/utils"

import { tint, toneColor } from "../openuiKit"
import {
  badgeTone,
  formatText,
  numeric,
  truthy,
  type ColumnFormat,
  type TableConfig,
} from "./model"
import { cycleSort, type SqlTableState } from "./useSqlTable"

// ── SqlTable — the grid: sortable/resizable/pinnable headers, formatted cells ─

const PAD: Record<SqlTableState["density"], string> = {
  compact: "px-2 py-0.5",
  normal: "px-2 py-1.5",
  comfortable: "px-3 py-2.5",
}

/** Fallback width for a pinned column without an explicit one (sticky offsets need a number). */
const PIN_WIDTH = 160

/** Width of the checkbox / row-number gutters. */
const GUTTER = 36

/** A visible column with its source index in the page rows. */
export interface Col {
  name: string
  index: number
}

/** Props of the grid. */
export interface GridProps {
  cfg: TableConfig
  st: SqlTableState
  cols: Col[]
  rows: (string | null)[][]
  label: (c: string) => string
  color: string
  rowKey: (r: (string | null)[], i: number) => string
}

interface CellProps {
  v: string
  text: string
  arg: string | undefined
  max: number
}

/** Styled renderers for the formats that are more than text. */
const RENDERERS: Partial<Record<ColumnFormat["kind"], (p: CellProps) => ReactNode>> = {
  badge: ({ v, text, arg }) => {
    const c = toneColor(arg ?? badgeTone(v))
    return (
      <span
        className="rounded-full px-2 py-0.5 text-[11px] font-medium"
        style={{ color: c, background: tint(c, 14) }}
      >
        {text}
      </span>
    )
  },
  bool: ({ v, text }) => (
    <span style={{ color: truthy(v) ? "var(--ok)" : "var(--danger)" }}>{text}</span>
  ),
  code: ({ text }) => (
    <code className="rounded-sm bg-foreground/6 px-1 font-mono text-[11.5px]">{text}</code>
  ),
  link: ({ v, text }) =>
    /^https?:\/\//.test(v) ? (
      <a
        className="underline decoration-foreground/30 hover:decoration-foreground"
        href={v}
        target="_blank"
        rel="noreferrer"
      >
        {text}
      </a>
    ) : (
      text
    ),
  bar: ({ v, arg, max }) => {
    const n = numeric(v) ?? 0
    const pct = max > 0 ? Math.max(2, (Math.abs(n) / max) * 100) : 0
    return (
      <span className="flex items-center gap-2">
        <span className="h-1.5 flex-1 rounded-full bg-foreground/6">
          <span
            className="block h-full rounded-full"
            style={{ width: `${String(pct)}%`, background: toneColor(arg ?? "signal") }}
          />
        </span>
        <span className="w-12 text-right">{v}</span>
      </span>
    )
  },
}

/** Rendered cell content under the column's format. */
function Cell({
  v,
  fmt,
  max,
}: {
  v: string
  fmt: ColumnFormat | undefined
  max: number
}): ReactNode {
  const text = formatText(v, fmt)
  const render = fmt === undefined ? undefined : RENDERERS[fmt.kind]
  return render === undefined ? text : render({ v, text, arg: fmt?.arg, max })
}

/** Drag handle on a header's right edge. */
function Resizer({
  onResize,
  start,
}: {
  onResize: (w: number) => void
  start: () => number
}): ReactNode {
  const originRef = useRef<{ x: number; w: number } | null>(null)
  const down = (e: PointerEvent<HTMLSpanElement>): void => {
    e.stopPropagation()
    e.currentTarget.setPointerCapture(e.pointerId)
    originRef.current = { x: e.clientX, w: start() }
  }
  const move = (e: PointerEvent<HTMLSpanElement>): void => {
    if (originRef.current) onResize(originRef.current.w + e.clientX - originRef.current.x)
  }
  return (
    <span
      role="separator"
      aria-orientation="vertical"
      className="absolute top-0 right-0 h-full w-1.5 cursor-col-resize hover:bg-foreground/15"
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={() => {
        originRef.current = null
      }}
    />
  )
}

/** Sticky-left offsets for pinned columns, after the gutters. */
function pinOffsets(cols: Col[], st: SqlTableState, gutter: number): Map<string, number> {
  const out = new Map<string, number>()
  let left = gutter
  for (const c of cols) {
    if (!st.pinned.includes(c.name)) continue
    out.set(c.name, left)
    left += st.widths[c.name] ?? PIN_WIDTH
  }
  return out
}

/** Header row: select-all, sort indicators with rank, resize handles. */
function Head({
  p,
  offsets,
  gutter,
}: {
  p: GridProps
  offsets: Map<string, number>
  gutter: number
}): ReactNode {
  const { st, cfg } = p
  const keys = p.rows.map((r, i) => p.rowKey(r, i))
  const all = keys.length > 0 && keys.every((k) => st.selected.has(k))
  const toggleAll = (): void => {
    const next = new Map(st.selected)
    for (const [i, r] of p.rows.entries()) {
      const k = p.rowKey(r, i)
      if (all) next.delete(k)
      else next.set(k, r)
    }
    st.setSelected(next)
  }
  return (
    <tr>
      {cfg.selectable && (
        <th className="sticky left-0 z-20 bg-card" style={{ width: GUTTER }}>
          <input type="checkbox" aria-label="Select page" checked={all} onChange={toggleAll} />
        </th>
      )}
      {cfg.rowNumbers && (
        <th
          className="sticky z-20 bg-card text-[11px] opacity-40"
          style={{ width: GUTTER, left: cfg.selectable ? GUTTER : 0 }}
        >
          #
        </th>
      )}
      {p.cols.map((c) => {
        const rank = st.sort.findIndex((k) => k.column === c.name)
        const key = st.sort[rank]
        const left = offsets.get(c.name)
        const width = st.widths[c.name] ?? (left === undefined ? undefined : PIN_WIDTH)
        const style: CSSProperties = {
          width,
          minWidth: width,
          maxWidth: width,
          left,
          textAlign: cfg.align[c.name] ?? "left",
        }
        return (
          <th
            key={c.name}
            aria-sort={key ? (key.desc ? "descending" : "ascending") : "none"}
            className={cn(
              "group relative bg-card text-[11px] font-medium tracking-wider whitespace-nowrap uppercase select-none",
              PAD[st.density],
              left !== undefined && "sticky z-20",
            )}
            style={style}
          >
            <button
              type="button"
              className="inline-flex cursor-pointer items-center gap-1 uppercase"
              style={{ opacity: key ? 1 : 0.55 }}
              title="Click to sort, Shift+click to add a sort key"
              onClick={(e) => st.setSort(cycleSort(st.sort, c.name, e.shiftKey))}
            >
              {p.label(c.name)}
              {key ? (
                key.desc ? (
                  <ArrowDown className="size-3" style={{ color: p.color }} />
                ) : (
                  <ArrowUp className="size-3" style={{ color: p.color }} />
                )
              ) : (
                <ChevronsUpDown className="size-3 opacity-0 group-hover:opacity-60" />
              )}
              {key && st.sort.length > 1 && (
                <span className="text-[9px]" style={{ color: p.color }}>
                  {String(rank + 1)}
                </span>
              )}
            </button>
            <Resizer start={() => width ?? 140} onResize={(w) => st.setWidth(c.name, w)} />
          </th>
        )
      })}
      {gutter < 0 && <th />}
    </tr>
  )
}

/** The scrolling table body + sticky header. */
export function Grid(p: GridProps): ReactNode {
  const { st, cfg } = p
  const gutter = (cfg.selectable ? GUTTER : 0) + (cfg.rowNumbers ? GUTTER : 0)
  const offsets = pinOffsets(p.cols, st, gutter)
  const maxOf = (i: number): number =>
    Math.max(0, ...p.rows.map((r) => Math.abs(numeric(r[i] ?? "") ?? 0)))
  const maxes = new Map(
    p.cols.filter((c) => cfg.formats[c.name]?.kind === "bar").map((c) => [c.name, maxOf(c.index)]),
  )
  const toggle = (r: (string | null)[], i: number): void => {
    const next = new Map(st.selected)
    const k = p.rowKey(r, i)
    if (!next.delete(k)) next.set(k, r)
    st.setSelected(next)
  }
  return (
    <div className="overflow-auto rounded-lg" style={{ maxHeight: cfg.maxHeight }}>
      <table className="w-full border-separate border-spacing-0 text-[12.5px]">
        <thead className="sticky top-0 z-30">
          <Head p={p} offsets={offsets} gutter={gutter} />
        </thead>
        <tbody>
          {p.rows.map((r, i) => {
            const key = p.rowKey(r, i)
            const sel = st.selected.has(key)
            return (
              <tr
                key={key}
                className={cn(
                  "transition-colors hover:bg-foreground/6",
                  cfg.striped && "odd:bg-foreground/3",
                )}
                style={sel ? { background: tint(p.color, 12) } : undefined}
              >
                {cfg.selectable && (
                  <td className="sticky left-0 z-10 bg-card text-center" style={{ width: GUTTER }}>
                    <input
                      type="checkbox"
                      aria-label="Select row"
                      checked={sel}
                      onChange={() => toggle(r, i)}
                    />
                  </td>
                )}
                {cfg.rowNumbers && (
                  <td
                    className="sticky z-10 bg-card text-center text-[11px] tabular-nums opacity-40"
                    style={{ left: cfg.selectable ? GUTTER : 0 }}
                  >
                    {String(st.offset + i + 1)}
                  </td>
                )}
                {p.cols.map((c) => {
                  const v = r[c.index] ?? null
                  const left = offsets.get(c.name)
                  const width = st.widths[c.name] ?? (left === undefined ? undefined : PIN_WIDTH)
                  const fmt = cfg.formats[c.name]
                  const align =
                    cfg.align[c.name] ??
                    (fmt &&
                    ["number", "integer", "percent", "currency", "bytes", "duration"].includes(
                      fmt.kind,
                    )
                      ? "right"
                      : "left")
                  return (
                    <td
                      key={c.name}
                      className={cn(
                        PAD[st.density],
                        "tabular-nums",
                        cfg.wrap ? "wrap-break-word" : "max-w-80 truncate",
                        left !== undefined && "sticky z-10 bg-card",
                      )}
                      style={{ width, minWidth: width, maxWidth: width, left, textAlign: align }}
                      title={v ?? "NULL"}
                    >
                      {v === null ? (
                        <span className="italic opacity-35">NULL</span>
                      ) : (
                        <Cell v={v} fmt={fmt} max={maxes.get(c.name) ?? 0} />
                      )}
                    </td>
                  )
                })}
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
