import { Columns3, Download, Filter, Pin, PinOff, Plus, Rows3, Search, Send, X } from "lucide-react"
import { useEffect, useRef, useState, type ReactNode } from "react"

import type { FilterOp } from "@/lib/api/generated/types.gen"
import { cn } from "@/lib/utils"

import { tint } from "../openuiKit"
import { OPS, type Density, type TableConfig } from "./model"
import type { FilterRow, SqlTableState } from "./useSqlTable"

// ── SqlTable — toolbar (search, filters, columns, density, export, send) ─────

const ICON_BTN =
  "flex h-7 items-center gap-1.5 rounded-md px-2 text-[12px] transition-colors hover:bg-foreground/8 disabled:opacity-35"

const FIELD = "h-7 rounded-md bg-foreground/5 px-2 text-[12px] outline-none focus:bg-foreground/8"

const DENSITY_NEXT: Record<Density, Density> = {
  compact: "normal",
  normal: "comfortable",
  comfortable: "compact",
}

const MENU_ROW =
  "flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-[12px] hover:bg-foreground/8"

/** Column visibility + pinning menu (a light popover; closes on outside click). */
function ColumnsMenu({
  columns,
  label,
  st,
}: {
  columns: string[]
  label: (c: string) => string
  st: SqlTableState
}): ReactNode {
  const [open, setOpen] = useState(false)
  const boxRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent): void => {
      if (!(e.target instanceof Node) || !boxRef.current?.contains(e.target)) setOpen(false)
    }
    document.addEventListener("mousedown", onDown)
    return () => document.removeEventListener("mousedown", onDown)
  }, [open])
  return (
    <div ref={boxRef} className="relative">
      <button
        type="button"
        className={ICON_BTN}
        aria-label="Columns"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <Columns3 className="size-3.5" />
        <span>Columns</span>
        {st.hidden.size > 0 && (
          <span className="opacity-60">({String(st.hidden.size)} hidden)</span>
        )}
      </button>
      {open && (
        <div className="absolute right-0 z-40 mt-1 max-h-80 min-w-56 overflow-auto rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10">
          <div className="px-2 py-1 text-[11px] opacity-50">Show · pin to the left</div>
          {columns.map((c) => (
            <div key={c} className="flex items-center gap-1">
              <label className={MENU_ROW}>
                <input
                  type="checkbox"
                  checked={!st.hidden.has(c)}
                  onChange={() => st.toggleHidden(c)}
                />
                <span className="truncate">{label(c)}</span>
              </label>
              <button
                type="button"
                className="rounded-md p-1 hover:bg-foreground/8"
                aria-label={`Pin ${label(c)}`}
                aria-pressed={st.pinned.includes(c)}
                onClick={() => st.togglePinned(c)}
              >
                {st.pinned.includes(c) ? (
                  <Pin className="size-3.5" />
                ) : (
                  <PinOff className="size-3.5 opacity-40" />
                )}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

/** One filter row: column, operator, operand, remove. */
function FilterLine({
  row,
  columns,
  label,
  onChange,
  onRemove,
}: {
  row: FilterRow
  columns: string[]
  label: (c: string) => string
  onChange: (r: FilterRow) => void
  onRemove: () => void
}): ReactNode {
  const unary = OPS.find((o) => o.op === row.op)?.unary === true
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <select
        className={FIELD}
        aria-label="Filter column"
        value={row.column}
        onChange={(e) => onChange({ ...row, column: e.target.value })}
      >
        {columns.map((c) => (
          <option key={c} value={c}>
            {label(c)}
          </option>
        ))}
      </select>
      <select
        className={FIELD}
        aria-label="Filter operator"
        value={row.op}
        onChange={(e) => onChange({ ...row, op: e.target.value as FilterOp })}
      >
        {OPS.map((o) => (
          <option key={o.op} value={o.op}>
            {o.label}
          </option>
        ))}
      </select>
      {!unary && (
        <input
          className={cn(FIELD, "w-40")}
          aria-label="Filter value"
          placeholder="value"
          value={row.value}
          onChange={(e) => onChange({ ...row, value: e.target.value })}
        />
      )}
      <button type="button" className={ICON_BTN} aria-label="Remove filter" onClick={onRemove}>
        <X className="size-3.5" />
      </button>
    </div>
  )
}

/** The filter builder panel (shown under the toolbar while open). */
export function FilterPanel({
  columns,
  label,
  st,
}: {
  columns: string[]
  label: (c: string) => string
  st: SqlTableState
}): ReactNode {
  const add = (): void =>
    st.setFilters([
      ...st.filters,
      { id: Date.now(), column: columns[0] ?? "", op: "contains", value: "" },
    ])
  return (
    <div className="flex flex-col gap-1.5 rounded-lg bg-foreground/3 p-2">
      {st.filters.map((f) => (
        <FilterLine
          key={f.id}
          row={f}
          columns={columns}
          label={label}
          onChange={(r) => st.setFilters(st.filters.map((x) => (x.id === f.id ? r : x)))}
          onRemove={() => st.setFilters(st.filters.filter((x) => x.id !== f.id))}
        />
      ))}
      <div className="flex gap-1.5">
        <button type="button" className={ICON_BTN} onClick={add}>
          <Plus className="size-3.5" /> Add filter
        </button>
        {st.filters.length > 0 && (
          <button type="button" className={ICON_BTN} onClick={() => st.setFilters([])}>
            Clear all
          </button>
        )}
      </div>
    </div>
  )
}

/** Props of the toolbar. */
export interface ToolbarProps {
  cfg: TableConfig
  st: SqlTableState
  columns: string[]
  label: (c: string) => string
  color: string
  filtersOpen: boolean
  onToggleFilters: () => void
  onExport: () => void
  onSend: () => void
}

/** Search box, filter toggle, columns menu, density, Excel export and send. */
export function Toolbar(p: ToolbarProps): ReactNode {
  const { cfg, st } = p
  const active = st.filters.length
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {cfg.search && (
        <label className="flex h-7 min-w-44 flex-1 items-center gap-1.5 rounded-md bg-foreground/5 px-2 focus-within:bg-foreground/8">
          <Search className="size-3.5 opacity-50" />
          <input
            className="w-full bg-transparent text-[12px] outline-none"
            placeholder="Search all columns…"
            aria-label="Search"
            value={st.search}
            onChange={(e) => st.setSearch(e.target.value)}
          />
          {st.search !== "" && (
            <button type="button" aria-label="Clear search" onClick={() => st.setSearch("")}>
              <X className="size-3.5 opacity-60" />
            </button>
          )}
        </label>
      )}
      {cfg.filters && (
        <button
          type="button"
          className={ICON_BTN}
          style={p.filtersOpen || active > 0 ? { background: tint(p.color, 14) } : undefined}
          onClick={p.onToggleFilters}
        >
          <Filter className="size-3.5" /> Filters{active > 0 && ` (${String(active)})`}
        </button>
      )}
      <ColumnsMenu columns={p.columns} label={p.label} st={st} />
      <button
        type="button"
        className={ICON_BTN}
        aria-label="Row density"
        title={`Density: ${st.density}`}
        onClick={() => st.setDensity(DENSITY_NEXT[st.density])}
      >
        <Rows3 className="size-3.5" /> <span className="capitalize">{st.density}</span>
      </button>
      {cfg.exportCsv && (
        <button
          type="button"
          className={ICON_BTN}
          aria-label="Export to Excel"
          onClick={p.onExport}
        >
          <Download className="size-3.5" /> Excel
        </button>
      )}
      {cfg.send !== undefined && (
        <button
          type="button"
          className={cn(ICON_BTN, "font-medium")}
          style={{ background: tint(p.color, 18) }}
          disabled={st.selected.size === 0}
          onClick={p.onSend}
        >
          <Send className="size-3.5" /> {cfg.sendLabel}
          {st.selected.size > 0 && ` (${String(st.selected.size)})`}
        </button>
      )}
    </div>
  )
}
