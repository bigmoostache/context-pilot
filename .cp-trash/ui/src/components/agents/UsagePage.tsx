import { useMemo, useState } from "react"
import { ArrowDownRight, ArrowUpRight, Coins, Hash, TrendingUp } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Table,
  TableBody,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { agents, usagePoints, USAGE_RATES } from "@/lib/mock"
import type { Agent, UsageUnit } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Cost & token analytics — the fleet "Usage" page. A filterable, forecastable
 * dashboard over the 12-month usage history.
 *
 * Controls: agent filter · date range · unit lens ($ / tokens). The data is
 * always bucketed by **calendar month** and always decomposed into the three
 * canonical sections — cache **hits**, **misses** (input), and **output** — so
 * the breakdown is consistent across every card, bar and table cell. The
 * current (partial) month is **forecast** to its full-month run-rate.
 *
 * Design-only: figures come from {@link usagePoints} mock data; CSS-only charts.
 */

// The three canonical sections, with their consistent identity everywhere.
type Section = "hit" | "miss" | "output"
const SECTIONS: { id: Section; label: string; accent: string; blurb: string }[] = [
  { id: "hit", label: "Cache hits", accent: "var(--interactive)", blurb: "Cache-read tokens" },
  { id: "miss", label: "Misses", accent: "var(--warn)", blurb: "Input / uncached tokens" },
  { id: "output", label: "Output", accent: "var(--signal)", blurb: "Generated tokens" },
]

const RANGES = [
  { id: 3, label: "3M" },
  { id: 6, label: "6M" },
  { id: 12, label: "12M" },
] as const
type RangeId = (typeof RANGES)[number]["id"]

/** One bucket's three sections, already converted to the active unit. */
interface Cell {
  hit: number
  miss: number
  output: number
  total: number
}

function toCell(p: { hitTokens: number; missTokens: number; outputTokens: number }, unit: UsageUnit): Cell {
  const hit = unit === "usd" ? p.hitTokens * USAGE_RATES.hit : p.hitTokens
  const miss = unit === "usd" ? p.missTokens * USAGE_RATES.miss : p.missTokens
  const output = unit === "usd" ? p.outputTokens * USAGE_RATES.output : p.outputTokens
  return { hit, miss, output, total: hit + miss + output }
}

const addCell = (a: Cell, b: Cell): Cell => ({
  hit: a.hit + b.hit,
  miss: a.miss + b.miss,
  output: a.output + b.output,
  total: a.total + b.total,
})
const ZERO: Cell = { hit: 0, miss: 0, output: 0, total: 0 }

const monthLabel = (key: string) => {
  const [y, m] = key.split("-").map(Number)
  const names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
  return { short: names[m - 1], full: `${names[m - 1]} ${y}` }
}

export function UsagePage() {
  const [unit, setUnit] = useState<UsageUnit>("usd")
  const [range, setRange] = useState<RangeId>(6)
  const [agentId, setAgentId] = useState<string>("all")

  const fmt = (v: number) =>
    unit === "usd"
      ? `$${v < 10 ? v.toFixed(2) : v < 1000 ? v.toFixed(0) : `${(v / 1000).toFixed(1)}K`}`
      : v >= 1e6
        ? `${(v / 1e6).toFixed(2)}M`
        : v >= 1e3
          ? `${(v / 1e3).toFixed(0)}K`
          : `${Math.round(v)}`

  // ── derive the visible series ───────────────────────────────────
  const { buckets, totals, forecast, perAgent } = useMemo(() => {
    const filtered = usagePoints.filter((p) => agentId === "all" || p.agentId === agentId)

    // unique months, newest last; take the trailing `range`
    const allMonths = [...new Set(usagePoints.map((p) => p.month))].sort()
    const months = allMonths.slice(-range)

    const buckets = months.map((month) => {
      const pts = filtered.filter((p) => p.month === month)
      const cell = pts.reduce<Cell>((acc, p) => addCell(acc, toCell(p, unit)), ZERO)
      const partial = pts.some((p) => p.partial)
      const elapsed = pts.find((p) => p.partial)?.elapsed ?? 1
      return { month, ...monthLabel(month), cell, partial, elapsed }
    })

    const totals = buckets.reduce<Cell>((acc, b) => addCell(acc, b.cell), ZERO)

    // forecast the partial (current) month to full run-rate
    const cur = buckets.find((b) => b.partial)
    const forecast = cur
      ? {
          month: cur.full,
          projected: cur.cell.total / (cur.elapsed || 1),
          mtd: cur.cell.total,
          cell: {
            hit: cur.cell.hit / (cur.elapsed || 1),
            miss: cur.cell.miss / (cur.elapsed || 1),
            output: cur.cell.output / (cur.elapsed || 1),
            total: cur.cell.total / (cur.elapsed || 1),
          } as Cell,
        }
      : null

    // per-agent share for the visible range
    const perAgent = agents
      .map((a) => {
        const cell = filtered
          .filter((p) => p.agentId === a.id && months.includes(p.month))
          .reduce<Cell>((acc, p) => addCell(acc, toCell(p, unit)), ZERO)
        return { agent: a, cell }
      })
      .filter((r) => r.cell.total > 0)
      .sort((x, y) => y.cell.total - x.cell.total)

    return { buckets, totals, forecast, perAgent }
  }, [unit, range, agentId])

  const peak = Math.max(
    ...buckets.map((b) => (b.partial && forecast ? forecast.cell.total : b.cell.total)),
    1,
  )

  return (
    <ScrollArea className="min-h-0 flex-1 bg-background">
      <div className="mx-auto flex w-full max-w-[1040px] flex-col gap-6 px-8 py-9">
        {/* header */}
        <header className="flex flex-col gap-1.5">
          <span className="label">Analytics</span>
          <h1 className="text-[24px] font-semibold tracking-tight text-foreground">Usage</h1>
          <p className="max-w-[600px] text-[13px] text-muted-foreground">
            Cost and token consumption across the fleet, by calendar month — always split into
            cache hits, misses and output. The current month is forecast to its full run-rate.
          </p>
        </header>

        {/* controls */}
        <div className="flex flex-wrap items-center gap-3">
          {/* agent filter */}
          <div className="flex items-center gap-0.5 rounded-lg border border-border bg-muted/60 p-0.5">
            <FilterChip active={agentId === "all"} onClick={() => setAgentId("all")} label="All agents" />
            {agents.map((a) => (
              <FilterChip
                key={a.id}
                active={agentId === a.id}
                onClick={() => setAgentId(a.id)}
                label={a.name}
                dot={accent(a)}
              />
            ))}
          </div>

          <div className="ml-auto flex items-center gap-2">
            {/* range */}
            <Segmented
              options={RANGES.map((r) => ({ id: String(r.id), label: r.label }))}
              value={String(range)}
              onChange={(v) => setRange(Number(v) as RangeId)}
            />
            {/* unit */}
            <Segmented
              options={[
                { id: "usd", label: "$", icon: Coins },
                { id: "tokens", label: "Tokens", icon: Hash },
              ]}
              value={unit}
              onChange={(v) => setUnit(v as UsageUnit)}
              accentActive
            />
          </div>
        </div>

        {/* hero stats — total + the 3 canonical sections */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <HeroCard
            label={unit === "usd" ? "Total spend" : "Total tokens"}
            value={fmt(totals.total)}
            sub={`${buckets.length} month${buckets.length === 1 ? "" : "s"}`}
            icon={unit === "usd" ? Coins : Hash}
            emphasis
          />
          {SECTIONS.map((s) => {
            const v = totals[s.id]
            const pct = totals.total > 0 ? (v / totals.total) * 100 : 0
            return (
              <HeroCard
                key={s.id}
                label={s.label}
                value={fmt(v)}
                sub={`${pct.toFixed(0)}% of total`}
                accent={s.accent}
              />
            )
          })}
        </div>

        {/* forecast callout */}
        {forecast && (
          <div className="flex items-center gap-3 rounded-xl border border-[var(--signal)]/30 bg-[var(--signal)]/[0.06] px-4 py-3">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-[var(--signal)]/15 text-[var(--signal)]">
              <TrendingUp className="size-[18px]" />
            </span>
            <div className="flex min-w-0 flex-1 flex-col leading-tight">
              <span className="text-[12.5px] font-medium text-foreground/90">
                {forecast.month} forecast
              </span>
              <span className="text-[11.5px] text-muted-foreground">
                {fmt(forecast.mtd)} so far · projected to{" "}
                <span className="font-semibold text-foreground/90">{fmt(forecast.projected)}</span> by month end
              </span>
            </div>
            <span className="shrink-0 font-mono text-[18px] font-semibold tabular-nums text-[var(--signal)]">
              {fmt(forecast.projected)}
            </span>
          </div>
        )}

        {/* stacked bar chart */}
        <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-5 card-shadow">
          <div className="flex items-center justify-between">
            <span className="text-[13px] font-semibold text-foreground/90">
              {unit === "usd" ? "Monthly spend" : "Monthly tokens"}
            </span>
            <Legend />
          </div>
          <div className="flex items-end gap-2.5 pt-2" style={{ height: 188 }}>
            {buckets.map((b) => {
              const showForecast = b.partial && forecast
              const fc = showForecast ? forecast.cell : null
              return (
                <div key={b.month} className="group flex min-w-0 flex-1 flex-col items-center gap-1.5">
                  <div className="relative flex w-full flex-1 items-end justify-center">
                    {/* forecast ghost (dashed) behind the actual bar */}
                    {fc && (
                      <div
                        className="absolute bottom-0 w-full max-w-[46px] rounded-md border border-dashed border-[var(--signal)]/50"
                        style={{ height: `${(fc.total / peak) * 100}%` }}
                        title={`Projected ${fmt(fc.total)}`}
                      />
                    )}
                    <Bar cell={b.cell} peak={peak} partial={b.partial} fmt={fmt} />
                  </div>
                  <span className={cn("text-[10.5px] tabular-nums", b.partial ? "font-semibold text-foreground/80" : "text-muted-foreground/70")}>
                    {b.short}
                  </span>
                </div>
              )
            })}
          </div>
        </section>

        {/* monthly table */}
        <section className="flex flex-col gap-2.5">
          <span className="text-[13px] font-semibold text-foreground/90">Monthly breakdown</span>
          <div className="overflow-hidden rounded-xl border border-border bg-card card-shadow">
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>Month</TableHead>
                  <Th accent={SECTIONS[0].accent}>Hits</Th>
                  <Th accent={SECTIONS[1].accent}>Misses</Th>
                  <Th accent={SECTIONS[2].accent}>Output</Th>
                  <TableHead className="text-right">Total</TableHead>
                  <TableHead className="text-right">Δ</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {buckets.map((b, i) => {
                  const prev = i > 0 ? buckets[i - 1].cell.total : null
                  const delta = prev && prev > 0 ? ((b.cell.total - prev) / prev) * 100 : null
                  return (
                    <TableRow key={b.month}>
                      <TableCell className="font-medium text-foreground/85">
                        <span className="flex items-center gap-2">
                          {b.full}
                          {b.partial && (
                            <span className="rounded-full bg-[var(--signal)]/14 px-1.5 py-px text-[9.5px] font-medium text-[var(--signal)]">
                              current
                            </span>
                          )}
                        </span>
                      </TableCell>
                      <Td>{fmt(b.cell.hit)}</Td>
                      <Td>{fmt(b.cell.miss)}</Td>
                      <Td>{fmt(b.cell.output)}</Td>
                      <TableCell className="text-right font-semibold tabular-nums text-foreground/90">
                        {fmt(b.cell.total)}
                      </TableCell>
                      <TableCell className="text-right">
                        <Delta pct={delta} />
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
              <TableFooter>
                <TableRow className="hover:bg-transparent">
                  <TableCell className="font-semibold text-foreground/90">Total</TableCell>
                  <Td bold>{fmt(totals.hit)}</Td>
                  <Td bold>{fmt(totals.miss)}</Td>
                  <Td bold>{fmt(totals.output)}</Td>
                  <TableCell className="text-right font-semibold tabular-nums text-foreground">
                    {fmt(totals.total)}
                  </TableCell>
                  <TableCell />
                </TableRow>
              </TableFooter>
            </Table>
          </div>
        </section>

        {/* per-agent breakdown (only meaningful when not already filtered to one) */}
        {agentId === "all" && perAgent.length > 1 && (
          <section className="flex flex-col gap-2.5">
            <span className="text-[13px] font-semibold text-foreground/90">By agent</span>
            <div className="flex flex-col gap-2.5 rounded-xl border border-border bg-card p-5 card-shadow">
              {perAgent.map(({ agent, cell }) => {
                const share = totals.total > 0 ? (cell.total / totals.total) * 100 : 0
                return (
                  <button
                    key={agent.id}
                    onClick={() => setAgentId(agent.id)}
                    className="group flex flex-col gap-1.5 rounded-lg px-1 py-1 text-left transition-colors hover:bg-muted/40"
                  >
                    <div className="flex items-center gap-2 text-[12.5px]">
                      <span className="size-2 shrink-0 rounded-full" style={{ background: accent(agent) }} />
                      <span className="font-medium text-foreground/85">{agent.name}</span>
                      <span className="ml-auto font-semibold tabular-nums text-foreground/90">{fmt(cell.total)}</span>
                      <span className="w-10 text-right text-[11px] tabular-nums text-muted-foreground/65">
                        {share.toFixed(0)}%
                      </span>
                    </div>
                    {/* proportional stacked share bar */}
                    <div className="flex h-2 w-full overflow-hidden rounded-full bg-muted">
                      {SECTIONS.map((s) => (
                        <span
                          key={s.id}
                          className="fill-sweep h-full"
                          style={{
                            width: `${cell.total > 0 ? (cell[s.id] / cell.total) * 100 : 0}%`,
                            background: s.accent,
                          }}
                        />
                      ))}
                    </div>
                  </button>
                )
              })}
            </div>
          </section>
        )}
      </div>
    </ScrollArea>
  )
}

const accent = (a: Agent) =>
  ({ signal: "var(--signal)", interactive: "var(--interactive)", ok: "var(--ok)", warn: "var(--warn)", danger: "var(--danger)" })[a.accent]

// ── chart bar ─────────────────────────────────────────────────────
function Bar({
  cell,
  peak,
  partial,
  fmt,
}: {
  cell: Cell
  peak: number
  partial?: boolean
  fmt: (v: number) => string
}) {
  const h = (cell.total / peak) * 100
  return (
    <div
      className={cn("relative z-[1] w-full max-w-[46px] overflow-hidden rounded-md transition-transform group-hover:scale-[1.03]", partial && "opacity-95")}
      style={{ height: `${h}%` }}
      title={`${fmt(cell.total)}`}
    >
      {/* output (top) → miss → hit (bottom), matching legend order top-down */}
      <Seg frac={cell.output / cell.total} color="var(--signal)" />
      <Seg frac={cell.miss / cell.total} color="var(--warn)" />
      <Seg frac={cell.hit / cell.total} color="var(--interactive)" />
    </div>
  )
}

function Seg({ frac, color }: { frac: number; color: string }) {
  if (!isFinite(frac) || frac <= 0) return null
  return <div style={{ height: `${frac * 100}%`, background: color }} />
}

function Legend() {
  return (
    <div className="flex items-center gap-3">
      {SECTIONS.map((s) => (
        <span key={s.id} className="flex items-center gap-1.5 text-[10.5px] text-muted-foreground" title={s.blurb}>
          <span className="size-2 rounded-[3px]" style={{ background: s.accent }} />
          {s.label}
        </span>
      ))}
    </div>
  )
}

// ── controls ──────────────────────────────────────────────────────
function FilterChip({
  active,
  onClick,
  label,
  dot,
}: {
  active: boolean
  onClick: () => void
  label: string
  dot?: string
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12px] font-medium transition-all",
        active ? "bg-card text-foreground card-shadow" : "text-muted-foreground hover:text-foreground",
      )}
    >
      {dot && <span className="size-2 rounded-full" style={{ background: dot }} />}
      {label}
    </button>
  )
}

function Segmented({
  options,
  value,
  onChange,
  accentActive,
}: {
  options: { id: string; label: string; icon?: typeof Coins }[]
  value: string
  onChange: (v: string) => void
  accentActive?: boolean
}) {
  return (
    <div className="flex items-center gap-0.5 rounded-lg border border-border bg-muted/60 p-0.5">
      {options.map((o) => {
        const on = o.id === value
        return (
          <button
            key={o.id}
            onClick={() => onChange(o.id)}
            className={cn(
              "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12px] font-medium transition-all",
              on
                ? accentActive
                  ? "bg-[var(--interactive)] text-[var(--primary-foreground)]"
                  : "bg-card text-foreground card-shadow"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {o.icon && <o.icon className="size-3.5" />}
            {o.label}
          </button>
        )
      })}
    </div>
  )
}

// ── stat cards ────────────────────────────────────────────────────
function HeroCard({
  label,
  value,
  sub,
  icon: Icon,
  accent,
  emphasis,
}: {
  label: string
  value: string
  sub: string
  icon?: typeof Coins
  accent?: string
  emphasis?: boolean
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-1.5 rounded-xl border bg-card p-4 card-shadow",
        emphasis ? "border-[var(--interactive)]/35" : "border-border",
      )}
    >
      <div className="flex items-center gap-2">
        {accent && <span className="size-2.5 rounded-[3px]" style={{ background: accent }} />}
        {Icon && <Icon className="size-3.5 text-muted-foreground/70" />}
        <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      </div>
      <span className="font-mono text-[22px] font-semibold tabular-nums tracking-tight text-foreground">{value}</span>
      <span className="text-[10.5px] text-muted-foreground/65">{sub}</span>
    </div>
  )
}

// ── table helpers ─────────────────────────────────────────────────
function Th({ accent, children }: { accent: string; children: React.ReactNode }) {
  return (
    <TableHead className="text-right">
      <span className="flex items-center justify-end gap-1.5">
        <span className="size-1.5 rounded-full" style={{ background: accent }} />
        {children}
      </span>
    </TableHead>
  )
}

function Td({ children, bold }: { children: React.ReactNode; bold?: boolean }) {
  return (
    <TableCell className={cn("text-right tabular-nums", bold ? "font-semibold text-foreground/90" : "text-muted-foreground")}>
      {children}
    </TableCell>
  )
}

function Delta({ pct }: { pct: number | null }) {
  if (pct == null) return <span className="text-[11px] text-muted-foreground/40">—</span>
  const up = pct >= 0
  return (
    <span
      className={cn(
        "inline-flex items-center gap-0.5 text-[11px] font-medium tabular-nums",
        up ? "text-[var(--warn)]" : "text-[var(--ok)]",
      )}
    >
      {up ? <ArrowUpRight className="size-3" /> : <ArrowDownRight className="size-3" />}
      {Math.abs(pct).toFixed(0)}%
    </span>
  )
}
