import { keepPreviousData, useQuery } from "@tanstack/react-query"
import { useEffect, useMemo, useState } from "react"

import { fetchSqlPage } from "@/lib/api"
import type { FilterOp, SqlPage, SqlView } from "@/lib/api/generated/types.gen"

import type { Density, TableConfig } from "./model"

// ── SqlTable — interactive state + the server query it drives ─────────────────
//
// Sort, search and filters are sent to the agent as a `SqlView`, so they apply
// to the whole result, not just the visible page. Column visibility, pinning,
// widths, density and selection are view-only and never leave the browser.

/** One editable filter row (`value` is ignored by unary operators). */
export interface FilterRow {
  id: number
  column: string
  op: FilterOp
  value: string
}

/** A sort key as the UI holds it. */
export interface SortKey {
  column: string
  desc: boolean
}

/** Debounce delay for search and filter typing. */
const DEBOUNCE_MS = 300

/** `value` once it has stopped changing for `ms`. */
function useDebounced<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value)
  useEffect(() => {
    const t = window.setTimeout(() => setSettled(value), ms)
    return () => window.clearTimeout(t)
  }, [value, ms])
  return settled
}

/** The wire view for the current sort/search/filter state (blank filters dropped). */
/** Operators that take no operand. */
const unary = (op: FilterOp): boolean => op === "is_null" || op === "not_null"

function toView(sort: SortKey[], search: string, filters: FilterRow[]): SqlView {
  return {
    sort,
    search: search.trim() === "" ? null : search.trim(),
    filters: filters
      .filter((f) => f.column !== "" && (unary(f.op) || f.value.trim() !== ""))
      .map((f) => ({ column: f.column, op: f.op, value: unary(f.op) ? null : f.value })),
  }
}

/** Next sort state after clicking `column` (shift = add as a secondary key). */
export function cycleSort(sort: SortKey[], column: string, multi: boolean): SortKey[] {
  const cur = sort.find((k) => k.column === column)
  const rest = sort.filter((k) => k.column !== column)
  // asc → desc → off
  const next =
    cur === undefined ? { column, desc: false } : cur.desc ? undefined : { column, desc: true }
  if (!multi) return next ? [next] : []
  return next ? (cur ? sort.map((k) => (k.column === column ? next : k)) : [...sort, next]) : rest
}

/** Everything the SqlTable renders from. */
export interface SqlTableState {
  page: ReturnType<typeof useQuery<SqlPage>>
  offset: number
  setOffset: (o: number) => void
  pageSize: number
  setPageSize: (n: number) => void
  sort: SortKey[]
  setSort: (s: SortKey[]) => void
  search: string
  setSearch: (s: string) => void
  filters: FilterRow[]
  setFilters: (f: FilterRow[]) => void
  hidden: Set<string>
  toggleHidden: (c: string) => void
  pinned: string[]
  togglePinned: (c: string) => void
  widths: Record<string, number>
  setWidth: (c: string, w: number) => void
  density: Density
  setDensity: (d: Density) => void
  selected: Map<string, (string | null)[]>
  setSelected: (m: Map<string, (string | null)[]>) => void
}

/** State + query for one SqlTable block. */
export function useSqlTable(fleetId: string | undefined, cfg: TableConfig): SqlTableState {
  const [offset, setOffset] = useState(0)
  const [pageSizeState, setPageSizeState] = useState(cfg.pageSize)
  const [sortState, setSortState] = useState<SortKey[]>(cfg.sort)
  const [search, setSearch] = useState("")
  const [filters, setFilters] = useState<FilterRow[]>([])
  const [hidden, setHidden] = useState(() => new Set(cfg.hidden))
  const [pinned, setPinned] = useState(cfg.pinned)
  const [widths, setWidths] = useState(cfg.widths)
  const [density, setDensity] = useState<Density>(cfg.density)
  const [selected, setSelected] = useState(() => new Map<string, (string | null)[]>())

  const view = useDebounced(
    useMemo(() => toView(sortState, search, filters), [sortState, search, filters]),
    DEBOUNCE_MS,
  )
  const viewKey = JSON.stringify(view)
  // A new view restarts at the first page.
  const [lastKey, setLastKey] = useState(viewKey)
  if (lastKey !== viewKey) {
    setLastKey(viewKey)
    setOffset(0)
  }

  const page = useQuery({
    queryKey: ["openui-sql", fleetId, cfg.sql, offset, pageSizeState, viewKey],
    queryFn: () =>
      fetchSqlPage(fleetId ?? "", { sql: cfg.sql, offset, limit: pageSizeState, view }),
    enabled: fleetId !== undefined && cfg.sql.length > 0,
    placeholderData: keepPreviousData,
    retry: false,
    staleTime: 30_000,
  })

  return {
    page,
    offset,
    setOffset,
    pageSize: pageSizeState,
    setPageSize: (n) => {
      setPageSizeState(n)
      setOffset(0)
    },
    sort: sortState,
    setSort: setSortState,
    search,
    setSearch,
    filters,
    setFilters,
    hidden,
    toggleHidden: (c) => {
      const next = new Set(hidden)
      if (!next.delete(c)) next.add(c)
      setHidden(next)
    },
    pinned,
    togglePinned: (c) =>
      setPinned(pinned.includes(c) ? pinned.filter((x) => x !== c) : [...pinned, c]),
    widths,
    setWidth: (c, w) => setWidths({ ...widths, [c]: Math.max(48, Math.round(w)) }),
    density,
    setDensity,
    selected,
    setSelected,
  }
}
