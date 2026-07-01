import { useCallback, useMemo, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { fetchFsPreview } from "@/lib/api/finder"
import { parseCostTsv, computeSummary, culpritDistribution, costBreakdown, toolCostAttribution, crossTabToolCulprit, buildMarkdownReport } from "./parse"
import { DonutChart, HBarChart, CostTimeline, fmtDollar, fmtTokens } from "./charts"

/**
 * Cost Analysis dashboard — developer-only view that reads the per-tick
 * cost-tracking TSV and renders charts for cache efficiency, culprit
 * attribution, and spend breakdown.
 */
export function CostsView({ agentId }: { agentId: string }) {
  const { data, isLoading, error } = useQuery({
    queryKey: ["cost-tsv", agentId],
    queryFn: () => fetchFsPreview(agentId, ".context-pilot/logs/cost-tracking.tsv"),
    refetchInterval: 15_000,
    retry: 1,
  })

  const rows = useMemo(() => parseCostTsv(data?.content ?? ""), [data?.content])

  // ── Filters ───────────────────────────────────────────────────────────────
  const [tempoFilter, setTempoFilter] = useState<"all" | "0" | "1">("all")
  const [queueFilter, setQueueFilter] = useState<"all" | "0" | "1">("all")
  const [breakFilter, setBreakFilter] = useState<"all" | "0" | "1">("all")

  const filtered = useMemo(() => {
    let r = rows
    if (tempoFilter !== "all") r = r.filter((x) => (tempoFilter === "1") === x.tempoActive)
    if (queueFilter !== "all") r = r.filter((x) => (queueFilter === "1") === x.queueActive)
    if (breakFilter !== "all") r = r.filter((x) => (breakFilter === "1") === x.noPanelBroken)
    return r
  }, [rows, tempoFilter, queueFilter, breakFilter])

  const summary = useMemo(() => computeSummary(filtered), [filtered])
  const culprits = useMemo(() => culpritDistribution(filtered), [filtered])
  const costs = useMemo(() => costBreakdown(filtered), [filtered])
  const tools = useMemo(() => toolCostAttribution(filtered), [filtered])
  const crossTab = useMemo(() => crossTabToolCulprit(filtered), [filtered])

  const [copied, setCopied] = useState(false)
  const handleCopy = useCallback(() => {
    const md = buildMarkdownReport(filtered, rows.length, {
      tempo: tempoFilter,
      queue: queueFilter,
      noBreak: breakFilter,
    })
    navigator.clipboard.writeText(md).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    })
  }, [filtered, rows.length, tempoFilter, queueFilter, breakFilter])

  if (isLoading) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center text-muted-foreground">
        <span className="text-[13px]">Loading cost data…</span>
      </div>
    )
  }

  if (error || rows.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 text-muted-foreground">
        <span className="text-[15px] font-semibold">No cost data yet</span>
        <span className="max-w-sm text-center text-[12px]">
          Cost telemetry appears after the agent completes its first LLM tick.
          The file <code className="rounded bg-muted px-1 py-0.5 text-[11px]">.context-pilot/logs/cost-tracking.tsv</code> will
          be created automatically.
        </span>
      </div>
    )
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-6 py-6">
        {/* ── Header ──────────────────────────────────────────────── */}
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-[17px] font-bold tracking-tight text-foreground">Cost Analysis</h2>
            <p className="mt-0.5 text-[12px] text-muted-foreground">
              Per-tick cache efficiency and spend breakdown · {summary.totalTicks} ticks
              {(tempoFilter !== "all" || queueFilter !== "all" || breakFilter !== "all") && ` (filtered from ${rows.length})`}
            </p>
          </div>
          <button
            onClick={handleCopy}
            className="flex items-center gap-1.5 rounded-lg border border-border bg-card px-3 py-1.5 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            {copied ? "✓ Copied" : "Copy as Markdown"}
          </button>
        </div>

        {/* ── Filters ─────────────────────────────────────────────── */}
        <div className="flex flex-wrap items-center gap-4">
          <FilterGroup label="Tempo" value={tempoFilter} onChange={setTempoFilter} />
          <FilterGroup label="Queue" value={queueFilter} onChange={setQueueFilter} />
          <FilterGroup label="No break" value={breakFilter} onChange={setBreakFilter} />
        </div>

        {/* ── Summary cards ───────────────────────────────────────── */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <Card label="Total cost" value={fmtDollar(summary.totalCost)} />
          <Card label="LLM ticks" value={summary.totalTicks.toLocaleString()} />
          <Card label="Avg cost / tick" value={fmtDollar(summary.avgCostPerTick)} />
          <Card label="Cache hit rate" value={`${(summary.cacheHitRate * 100).toFixed(1)}%`} accent={summary.cacheHitRate > 0.5} />
        </div>

        {/* ── Row: ticks with/without cache breaks ────────────────── */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <Card label="Clean ticks" value={summary.ticksClean.toLocaleString()} sub="no panel broke" />
          <Card label="Break ticks" value={summary.ticksWithBreak.toLocaleString()} sub="cache broken" />
          <Card label="Hit tokens" value={fmtTokens(summary.totalHitTokens)} />
          <Card label="Miss tokens" value={fmtTokens(summary.totalMissTokens)} />
        </div>

        {/* ── Row: Culprit donut + Cost timeline ──────────────────── */}
        <div className="grid gap-6 lg:grid-cols-2">
          <Section>
            <DonutChart data={culprits} title="Cache break culprits" />
          </Section>
          <Section>
            <CostTimeline rows={rows} title="Cost per tick over time" />
          </Section>
        </div>

        {/* ── Row: Cost breakdown + Tool attribution ──────────────── */}
        <div className="grid gap-6 lg:grid-cols-2">
          <Section>
            <HBarChart data={costs} title="Cost by category" />
          </Section>
          <Section>
            <HBarChart data={tools} title="Top tools by associated cost" />
          </Section>
        </div>

        {/* ── Token distribution per tick (average) ────────────────── */}
        {filtered.length > 0 && <TokenDistribution rows={filtered} />}

        {/* ── API-reported token distribution (comparison) ─────────── */}
        {filtered.length > 0 && <ApiTokenDistribution rows={filtered} />}

        {/* ── Tool × Culprit cross-tab ────────────────────────────── */}
        {crossTab.tools.length > 0 && crossTab.culprits.length > 0 && (
          <CrossTabTable crossTab={crossTab} totalTicks={filtered.length} />
        )}
      </div>
    </div>
  )
}

// ── Micro-components ────────────────────────────────────────────────────────

function Card({
  label,
  value,
  sub,
  accent,
}: {
  label: string
  value: string
  sub?: string
  accent?: boolean
}) {
  return (
    <div className="flex flex-col gap-1 rounded-xl border border-border bg-card px-4 py-3 card-shadow">
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      <span
        className="text-[18px] font-bold tabular-nums tracking-tight"
        style={accent ? { color: "var(--ok, #4ade80)" } : undefined}
      >
        {value}
      </span>
      {sub && <span className="text-[10px] text-muted-foreground/70">{sub}</span>}
    </div>
  )
}

function Section({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-border bg-card p-5 card-shadow">
      {children}
    </div>
  )
}

type FilterValue = "all" | "0" | "1"

function FilterGroup({
  label,
  value,
  onChange,
}: {
  label: string
  value: FilterValue
  onChange: (v: FilterValue) => void
}) {
  const opts: { v: FilterValue; text: string }[] = [
    { v: "all", text: "All" },
    { v: "0", text: "Off" },
    { v: "1", text: "On" },
  ]
  return (
    <div className="flex items-center gap-1.5">
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      <div className="flex overflow-hidden rounded-lg border border-border">
        {opts.map((o) => (
          <button
            key={o.v}
            onClick={() => onChange(o.v)}
            className={`px-2.5 py-0.5 text-[11px] font-medium transition-colors ${
              value === o.v
                ? "bg-foreground text-background"
                : "bg-transparent text-muted-foreground hover:bg-muted"
            }`}
          >
            {o.text}
          </button>
        ))}
      </div>
    </div>
  )
}

/** HSL heat-map: 0 → transparent, low → green, high → red. */
function heatBg(count: number, max: number): string | undefined {
  if (count === 0 || max === 0) return undefined
  const t = count / max // 0..1
  const hue = 120 - t * 120 // 120 (green) → 0 (red)
  const sat = 50 + t * 15 // 50% → 65%
  const lit = 93 - t * 18 // 93% → 75%
  return `hsl(${hue}, ${sat}%, ${lit}%)`
}

/** Dark-mode heat-map — lower lightness, slightly higher saturation. */
function heatBgDark(count: number, max: number): string | undefined {
  if (count === 0 || max === 0) return undefined
  const t = count / max
  const hue = 120 - t * 120
  const sat = 40 + t * 20
  const lit = 20 + t * 10
  return `hsl(${hue}, ${sat}%, ${lit}%)`
}

import type { CrossTab, CostRow } from "./parse"

function CrossTabTable({ crossTab, totalTicks }: { crossTab: CrossTab; totalTicks: number }) {
  const [hover, setHover] = useState<{ tool: string; culprit: string } | null>(null)
  const isDark = document.documentElement.classList.contains("dark")
  const bgFn = isDark ? heatBgDark : heatBg

  // Find max cell value for heat-map normalization
  const maxCount = useMemo(() => {
    let mx = 0
    for (const v of crossTab.cells.values()) if (v > mx) mx = v
    return mx
  }, [crossTab.cells])

  return (
    <Section>
      <div className="flex items-baseline justify-between">
        <span className="text-[13px] font-semibold text-foreground/80">
          Tool × Culprit (occurrence count)
        </span>
        {/* Hover info bar */}
        {hover && (
          <span className="text-[11px] text-muted-foreground">
            <strong className="text-foreground/80">{hover.tool}</strong>
            {" × "}
            <strong className="text-foreground/80">{hover.culprit}</strong>
            {" = "}
            <strong className="tabular-nums text-foreground">
              {crossTab.cells.get(`${hover.tool}\t${hover.culprit}`) ?? 0}
            </strong>
          </span>
        )}
      </div>
      <div className="mt-3 overflow-x-auto">
        <table className="w-full border-collapse text-[11px]">
          <thead>
            <tr>
              <th className="sticky left-0 z-10 bg-card px-2 py-1.5 text-left font-medium text-muted-foreground">
                Tool
              </th>
              {crossTab.culprits.map((c) => (
                <th
                  key={c}
                  className={`px-2 py-1.5 text-center font-medium transition-colors ${
                    hover?.culprit === c ? "text-foreground" : "text-muted-foreground"
                  }`}
                >
                  {c}
                </th>
              ))}
              <th className="px-2 py-1.5 text-center font-semibold text-foreground/70">Total</th>
            </tr>
          </thead>
          <tbody>
            {crossTab.tools.map((tool) => {
              const rowTotal = crossTab.culprits.reduce(
                (s, c) => s + (crossTab.cells.get(`${tool}\t${c}`) ?? 0),
                0,
              )
              const isRowHovered = hover?.tool === tool
              return (
                <tr key={tool} className="border-t border-border/40">
                  <td
                    className={`sticky left-0 z-10 bg-card px-2 py-1 font-medium transition-colors ${
                      isRowHovered ? "text-foreground" : "text-foreground/80"
                    }`}
                  >
                    {tool}
                  </td>
                  {crossTab.culprits.map((c) => {
                    const v = crossTab.cells.get(`${tool}\t${c}`) ?? 0
                    const isActive = hover?.tool === tool && hover?.culprit === c
                    const isDimmed = hover !== null && !isActive && hover.tool !== tool && hover.culprit !== c
                    return (
                      <td
                        key={c}
                        className={`px-2 py-1 text-center tabular-nums transition-all duration-150 ${
                          isActive
                            ? "ring-2 ring-foreground/30 ring-inset font-bold text-foreground"
                            : isDimmed
                              ? "text-foreground/25"
                              : "text-foreground/70"
                        }`}
                        style={{
                          backgroundColor: v > 0 ? bgFn(v, maxCount) : undefined,
                          borderRadius: isActive ? "4px" : undefined,
                        }}
                        onMouseEnter={() => setHover({ tool, culprit: c })}
                        onMouseLeave={() => setHover(null)}
                      >
                        {v > 0 ? v : <span className="text-muted-foreground/30">·</span>}
                      </td>
                    )
                  })}
                  <td className="px-2 py-1 text-center tabular-nums font-semibold text-foreground/70">
                    {rowTotal}
                  </td>
                </tr>
              )
            })}
          </tbody>
          <tfoot>
            <tr className="border-t border-border">
              <td className="sticky left-0 z-10 bg-card px-2 py-1.5 font-semibold text-foreground/70">
                Total
              </td>
              {crossTab.culprits.map((c) => {
                const colTotal = crossTab.tools.reduce(
                  (s, t) => s + (crossTab.cells.get(`${t}\t${c}`) ?? 0),
                  0,
                )
                return (
                  <td
                    key={c}
                    className={`px-2 py-1.5 text-center tabular-nums font-semibold transition-colors ${
                      hover?.culprit === c ? "text-foreground" : "text-foreground/70"
                    }`}
                  >
                    {colTotal}
                  </td>
                )
              })}
              <td className="px-2 py-1.5 text-center tabular-nums font-bold text-foreground">
                {totalTicks}
              </td>
            </tr>
          </tfoot>
        </table>
      </div>
    </Section>
  )
}

function ApiTokenDistribution({ rows }: { rows: CostRow[] }) {
  if (rows.length === 0) return null

  const avgHit = Math.round(rows.reduce((s, r) => s + r.hitTokens, 0) / rows.length)
  const avgMiss = Math.round(rows.reduce((s, r) => s + r.missTokens, 0) / rows.length)
  const avgOut = Math.round(rows.reduce((s, r) => s + r.outTokens, 0) / rows.length)
  const total = avgHit + avgMiss + avgOut

  const segments = [
    { label: "Cache hit", value: avgHit, color: "var(--ok, #4ade80)" },
    { label: "Cache miss", value: avgMiss, color: "var(--danger, #ef4444)" },
    { label: "Output", value: avgOut, color: "#60a5fa" },
  ]

  return (
    <Section>
      <span className="text-[13px] font-semibold text-foreground/80">
        API-reported token layout ({rows.length} ticks)
      </span>
      <div className="mt-3 flex h-7 overflow-hidden rounded-lg">
        {segments.map((s) => (
          <div
            key={s.label}
            className="flex items-center justify-center text-[10px] font-medium text-white transition-all duration-500"
            style={{
              width: `${(s.value / total) * 100}%`,
              backgroundColor: s.color,
              minWidth: s.value > 0 ? "2%" : 0,
            }}
          >
            {(s.value / total) * 100 > 8 ? fmtTokens(s.value) : ""}
          </div>
        ))}
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
        {segments.map((s) => (
          <span key={s.label} className="flex items-center gap-1.5">
            <span className="inline-block size-2.5 rounded-sm" style={{ backgroundColor: s.color }} />
            {s.label}: {fmtTokens(s.value)} avg
          </span>
        ))}
      </div>
    </Section>
  )
}

function TokenDistribution({ rows }: { rows: CostRow[] }) {
  if (rows.length === 0) return null

  const avgBefore = Math.round(rows.reduce((s, r) => s + r.tokensBefore, 0) / rows.length)
  const avgCulprit = Math.round(rows.reduce((s, r) => s + r.tokensCulprit, 0) / rows.length)
  const avgAfter = Math.round(rows.reduce((s, r) => s + r.tokensAfter, 0) / rows.length)
  const total = avgBefore + avgCulprit + avgAfter

  const segments = [
    { label: "Before culprit", value: avgBefore, color: "var(--ok, #4ade80)" },
    { label: "Culprit panel", value: avgCulprit, color: "var(--danger, #ef4444)" },
    { label: "After culprit", value: avgAfter, color: "#60a5fa" },
  ]

  return (
    <Section>
      <span className="text-[13px] font-semibold text-foreground/80">
        Average token layout ({rows.length} ticks)
      </span>
      <div className="mt-3 flex h-7 overflow-hidden rounded-lg">
        {segments.map((s) => (
          <div
            key={s.label}
            className="flex items-center justify-center text-[10px] font-medium text-white transition-all duration-500"
            style={{
              width: `${(s.value / total) * 100}%`,
              backgroundColor: s.color,
              minWidth: s.value > 0 ? "2%" : 0,
            }}
          >
            {(s.value / total) * 100 > 8 ? fmtTokens(s.value) : ""}
          </div>
        ))}
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
        {segments.map((s) => (
          <span key={s.label} className="flex items-center gap-1.5">
            <span className="inline-block size-2.5 rounded-sm" style={{ backgroundColor: s.color }} />
            {s.label}: {fmtTokens(s.value)} avg
          </span>
        ))}
      </div>
    </Section>
  )
}
