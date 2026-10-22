import { ChevronFirst, ChevronLast, ChevronLeft, ChevronRight } from "lucide-react"
import { use, useState, type ReactNode } from "react"

import { cn } from "@/lib/utils"

import {
  FleetIdContext,
  OPENUI_SEND_EVENT,
  firstText,
  interpolate,
  tint,
  toneColor,
} from "../openuiKit"
import type { OpenUiNode } from "../openuiParse"
import { Grid, type Col } from "./Grid"
import { cells, parseConfig, toCsv, toObjects, type TableConfig } from "./model"
import { FilterPanel, Toolbar } from "./Toolbar"
import { useSqlTable, type SqlTableState } from "./useSqlTable"

// ── OpenUI Lang — `SqlTable`: a live, configurable data table over entity SQL ─
//
// Rows come one page at a time from `POST /api/fleet/{id}/sql`; the agent runs
// the query (read-only, single statement, deadline-bounded) on its bridge
// intake thread. Sort, search and filters travel as a `SqlView` and run in that
// query, so they cover the whole result. See ./model.ts for every prop.

const PAGER_BTN =
  "flex size-7 items-center justify-center rounded-md transition-colors enabled:hover:bg-foreground/8 disabled:opacity-30"

/** Page-size picker, "a–b of N", page x/y and first/prev/next/last. */
function Pager({
  cfg,
  st,
  total,
}: {
  cfg: TableConfig
  st: SqlTableState
  total: number
}): ReactNode {
  const { offset, pageSize: size } = st
  const end = Math.min(total, offset + size)
  const pages = Math.max(1, Math.ceil(total / size))
  const current = Math.floor(offset / size) + 1
  const last = (pages - 1) * size
  return (
    <div className="flex flex-wrap items-center justify-between gap-2 text-[12px] tabular-nums">
      <div className="flex items-center gap-2 opacity-80">
        {st.selected.size > 0 && (
          <button
            type="button"
            className="hover:underline"
            onClick={() => st.setSelected(new Map())}
          >
            {String(st.selected.size)} selected · clear
          </button>
        )}
        <label className="flex items-center gap-1.5">
          Rows
          <select
            className="h-7 rounded-md bg-foreground/5 px-1.5 outline-none"
            aria-label="Rows per page"
            value={size}
            onChange={(e) => st.setPageSize(Number(e.target.value))}
          >
            {cfg.pageSizes.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className={cn("flex items-center gap-1 opacity-80", st.page.isFetching && "opacity-50")}>
        <span className="mr-1">
          {total === 0
            ? "0 rows"
            : `${String(offset + 1)}–${String(end)} of ${total.toLocaleString()}`}
          {pages > 1 && ` · page ${String(current)}/${String(pages)}`}
        </span>
        <button
          type="button"
          className={PAGER_BTN}
          aria-label="First page"
          disabled={offset === 0}
          onClick={() => st.setOffset(0)}
        >
          <ChevronFirst className="size-4" />
        </button>
        <button
          type="button"
          className={PAGER_BTN}
          aria-label="Previous page"
          disabled={offset === 0}
          onClick={() => st.setOffset(Math.max(0, offset - size))}
        >
          <ChevronLeft className="size-4" />
        </button>
        <button
          type="button"
          className={PAGER_BTN}
          aria-label="Next page"
          disabled={end >= total}
          onClick={() => st.setOffset(offset + size)}
        >
          <ChevronRight className="size-4" />
        </button>
        <button
          type="button"
          className={PAGER_BTN}
          aria-label="Last page"
          disabled={end >= total}
          onClick={() => st.setOffset(last)}
        >
          <ChevronLast className="size-4" />
        </button>
      </div>
    </div>
  )
}

/** Placeholder rows while the first page loads. */
function Skeleton(): ReactNode {
  return (
    <div className="flex flex-col gap-1.5">
      {[0, 1, 2, 3].map((i) => (
        <div key={i} className="h-6 animate-pulse rounded-md bg-foreground/5" />
      ))}
    </div>
  )
}

/** The error text the API client attached, or a generic fallback. */
const errorText = (e: unknown): string =>
  e instanceof Error && e.message ? e.message : "Query failed"

/** Download `text` as a file. */
function download(name: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: "text/csv;charset=utf-8" }))
  const a = document.createElement("a")
  a.href = url
  a.download = name
  a.click()
  URL.revokeObjectURL(url)
}

/** Fill the `send` template: `{count}`, `{json}`, `{csv}`, `{ids}` (first column). */
function sendText(template: string, columns: string[], rows: (string | null)[][]): string {
  return interpolate(template, {
    count: String(rows.length),
    json: JSON.stringify(toObjects(columns, rows)),
    csv: toCsv(columns, rows),
    ids: rows.map((r) => r[0] ?? "NULL").join(", "),
  })
}

/** `SqlTable("SELECT …", …props)` — see ./model.ts for the full prop list. */
export function SqlTable({ node }: { node: OpenUiNode }): ReactNode {
  const fleetId = use(FleetIdContext)
  const [cfg] = useState(() => parseConfig(node, firstText(node)))
  const st = useSqlTable(fleetId, cfg)
  const [filtersOpen, setFiltersOpen] = useState(false)
  const [flash, setFlash] = useState<string | null>(null)
  const color = toneColor(node.props["tone"])
  const data = st.page.data
  const columns = data?.columns ?? []
  const rows = data ? cells(data.rows) : []
  const label = (c: string): string => cfg.labels[c] ?? c
  // Pinned columns first (in pin order), then the rest in result order.
  const visible: Col[] = columns
    .map((name, index) => ({ name, index }))
    .filter((c) => !st.hidden.has(c.name))
  const cols = [
    ...st.pinned.flatMap((n) => visible.filter((c) => c.name === n)),
    ...visible.filter((c) => !st.pinned.includes(c.name)),
  ]
  const rowKey = (r: (string | null)[], i: number): string =>
    r[0] === null || r[0] === undefined
      ? `#${String(st.offset + i)}`
      : `${r[0]}·${String(st.offset + i)}`

  const exportCsv = (): void => {
    if (!data) return
    const idx = cols.map((c) => c.index)
    download(
      `${cfg.title || "query"}.csv`,
      toCsv(
        cols.map((c) => label(c.name)),
        rows.map((r) => idx.map((i) => r[i] ?? null)),
      ),
    )
  }
  const send = (): void => {
    if (cfg.send === undefined || st.selected.size === 0) return
    const picked = Array.from(st.selected, ([, r]) => r)
    const detail = sendText(cfg.send, columns, picked)
    window.dispatchEvent(new CustomEvent(OPENUI_SEND_EVENT, { detail }))
    setFlash("Sent ✓")
    window.setTimeout(() => setFlash(null), 1600)
  }

  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div className="flex items-center gap-2">
        {cfg.title && <span className="text-[13px] font-semibold">{cfg.title}</span>}
        <code
          className="truncate rounded-md px-1.5 py-0.5 font-mono text-[11px] opacity-70"
          style={{ background: tint(color, 10) }}
          title={cfg.sql}
        >
          {cfg.sql}
        </code>
        {flash && (
          <span className="text-[12px]" style={{ color: "var(--ok)" }}>
            {flash}
          </span>
        )}
      </div>
      {cfg.toolbar && data && (
        <Toolbar
          cfg={cfg}
          st={st}
          columns={columns}
          label={label}
          color={color}
          filtersOpen={filtersOpen}
          onToggleFilters={() => setFiltersOpen(!filtersOpen)}
          onExport={exportCsv}
          onSend={send}
        />
      )}
      {filtersOpen && data && <FilterPanel columns={columns} label={label} st={st} />}
      {fleetId === undefined && <span className="text-[12px] opacity-60">No agent context.</span>}
      {st.page.isError && (
        <span
          className="rounded-md px-2 py-1 text-[12px]"
          style={{ color: "var(--danger)", background: tint("var(--danger)", 12) }}
        >
          {errorText(st.page.error)}
        </span>
      )}
      {st.page.isPending && fleetId !== undefined && <Skeleton />}
      {data && (
        <>
          <Grid
            cfg={cfg}
            st={st}
            cols={cols}
            rows={rows}
            label={label}
            color={color}
            rowKey={rowKey}
          />
          <Pager cfg={cfg} st={st} total={data.total} />
        </>
      )}
    </div>
  )
}
