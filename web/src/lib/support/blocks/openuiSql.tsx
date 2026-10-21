import { keepPreviousData, useQuery } from "@tanstack/react-query"
import { ChevronLeft, ChevronRight } from "lucide-react"
import { use, useState, type ReactNode } from "react"

import { fetchSqlPage } from "@/lib/api"
import { cn } from "@/lib/utils"

import { FleetIdContext, firstText, numProp, str, tint, toneColor } from "./openuiKit"
import type { OpenUiNode } from "./openuiParse"

// ── OpenUI Lang — `SqlTable`: a live, paginated view over the entity database ──
//
// `SqlTable("SELECT …", page_size: 25, title: "…")` renders one page at a time
// from `POST /api/fleet/{id}/sql`. The agent runs the query (read-only, single
// statement, deadline-bounded) on its bridge intake thread — never its main
// loop — so paging never stalls the conversation. The frontend only renders the
// rows it is handed and asks for the next offset.

/** Rows per page when the block does not say (the backend clamps to 1..200). */
const DEFAULT_PAGE = 25

/** Display text for one cell; `null` means SQL NULL. */
const cellText = (v: unknown): string | null =>
  v === null || v === undefined ? null : typeof v === "string" ? v : JSON.stringify(v)

/** The error text the API client attached, or a generic fallback. */
const errorText = (e: unknown): string =>
  e instanceof Error && e.message ? e.message : "Query failed"

/** Prev/next pager with the "a–b of N" readout. */
function Pager({
  offset,
  size,
  total,
  busy,
  onMove,
}: {
  offset: number
  size: number
  total: number
  busy: boolean
  onMove: (offset: number) => void
}): ReactNode {
  const end = Math.min(total, offset + size)
  const btn =
    "flex size-7 items-center justify-center rounded-md transition-colors enabled:hover:bg-foreground/8 disabled:opacity-30"
  return (
    <div className="flex items-center justify-end gap-2 text-[12px] tabular-nums opacity-80">
      <span className={cn(busy && "opacity-50")}>
        {total === 0 ? "0 rows" : `${String(offset + 1)}–${String(end)} of ${String(total)}`}
      </span>
      <button
        type="button"
        className={btn}
        aria-label="Previous page"
        disabled={offset === 0}
        onClick={() => onMove(Math.max(0, offset - size))}
      >
        <ChevronLeft className="size-4" />
      </button>
      <button
        type="button"
        className={btn}
        aria-label="Next page"
        disabled={end >= total}
        onClick={() => onMove(offset + size)}
      >
        <ChevronRight className="size-4" />
      </button>
    </div>
  )
}

/** One page of rows under a sticky header; NULL cells render muted. */
function Rows({ columns, rows }: { columns: string[]; rows: unknown[][] }): ReactNode {
  return (
    <div className="max-h-112 overflow-auto rounded-lg">
      <table className="w-full text-[12.5px]">
        <thead className="sticky top-0 bg-card">
          <tr>
            {columns.map((c) => (
              <th
                key={c}
                className="p-2 text-left text-[11px] font-medium tracking-wider whitespace-nowrap uppercase opacity-50"
              >
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className="transition-colors odd:bg-foreground/3 hover:bg-foreground/6">
              {r.map((v, j) => {
                const text = cellText(v)
                return (
                  <td
                    key={j}
                    className="max-w-80 truncate px-2 py-1.5 tabular-nums"
                    title={text ?? "NULL"}
                  >
                    {text ?? <span className="italic opacity-35">NULL</span>}
                  </td>
                )
              })}
            </tr>
          ))}
        </tbody>
      </table>
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

/** `SqlTable("SELECT …", page_size, title, tone)` — server-paginated query results. */
export function SqlTable({ node }: { node: OpenUiNode }): ReactNode {
  const fleetId = use(FleetIdContext)
  const sql = firstText(node)
  const size = numProp(node, "page_size") ?? DEFAULT_PAGE
  const [offset, setOffset] = useState(0)
  const page = useQuery({
    queryKey: ["openui-sql", fleetId, sql, offset, size],
    queryFn: () => fetchSqlPage(fleetId ?? "", { sql, offset, limit: size }),
    enabled: fleetId !== undefined && sql.length > 0,
    placeholderData: keepPreviousData,
    retry: false,
    staleTime: 30_000,
  })
  const color = toneColor(node.props["tone"])
  const title = str(node.props["title"])
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        {title && <span className="text-[13px] font-semibold">{title}</span>}
        <code
          className="truncate rounded-md px-1.5 py-0.5 font-mono text-[11px] opacity-70"
          style={{ background: tint(color, 10) }}
          title={sql}
        >
          {sql}
        </code>
      </div>
      {fleetId === undefined && <span className="text-[12px] opacity-60">No agent context.</span>}
      {page.isError && (
        <span
          className="rounded-md px-2 py-1 text-[12px]"
          style={{ color: "var(--danger)", background: tint("var(--danger)", 12) }}
        >
          {errorText(page.error)}
        </span>
      )}
      {page.isPending && fleetId !== undefined && <Skeleton />}
      {page.data && (
        <>
          <Rows columns={page.data.columns} rows={page.data.rows} />
          <Pager
            offset={offset}
            size={size}
            total={page.data.total}
            busy={page.isFetching}
            onMove={setOffset}
          />
        </>
      )}
    </div>
  )
}
