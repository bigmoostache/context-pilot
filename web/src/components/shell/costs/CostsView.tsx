import { useCallback, useMemo, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { fetchFsPreview } from "@/lib/api/finder"
import {
  parseCostTsv,
  computeSummary,
  culpritDistribution,
  costBreakdown,
  toolCostAttribution,
  culpritCostAttribution,
  maxFreezePerCulprit,
  crossTabToolCulprit,
  buildMarkdownReport,
} from "./parse"
import { DonutChart, HBarChart, CostTimeline } from "./charts"
import { fmtDollar, fmtTokens } from "./format"
import { CrossTabTable, TokenDistribution, ApiTokenDistribution } from "./tables"

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
  const [breakKindFilter, setBreakKindFilter] = useState<string>("all")

  const filtered = useMemo(() => {
    let r = rows
    if (tempoFilter !== "all") r = r.filter((x) => (tempoFilter === "1") === x.tempoActive)
    if (queueFilter !== "all") r = r.filter((x) => (queueFilter === "1") === x.queueActive)
    if (breakKindFilter !== "all") r = r.filter((x) => x.breakKind === breakKindFilter)
    return r
  }, [rows, tempoFilter, queueFilter, breakKindFilter])

  const summary = useMemo(() => computeSummary(filtered), [filtered])
  const culprits = useMemo(() => culpritDistribution(filtered), [filtered])
  const costs = useMemo(() => costBreakdown(filtered), [filtered])
  const tools = useMemo(() => toolCostAttribution(filtered), [filtered])
  const culpritCosts = useMemo(() => culpritCostAttribution(filtered), [filtered])
  const maxFreezes = useMemo(() => maxFreezePerCulprit(filtered), [filtered])
  const crossTab = useMemo(() => crossTabToolCulprit(filtered), [filtered])

  const [copied, setCopied] = useState(false)
  const handleCopy = useCallback(() => {
    const md = buildMarkdownReport(filtered, rows.length, {
      tempo: tempoFilter,
      queue: queueFilter,
      breakKind: breakKindFilter,
    })
    void navigator.clipboard.writeText(md).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    })
  }, [filtered, rows.length, tempoFilter, queueFilter, breakKindFilter])

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
          Cost telemetry appears after the agent completes its first LLM tick. The file{" "}
          <code className="rounded bg-muted px-1 py-0.5 text-[11px]">
            .context-pilot/logs/cost-tracking.tsv
          </code>{" "}
          will be created automatically.
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
              {(tempoFilter !== "all" || queueFilter !== "all" || breakKindFilter !== "all") &&
                ` (filtered from ${rows.length})`}
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
          <BreakKindFilter value={breakKindFilter} onChange={setBreakKindFilter} />
        </div>

        {/* ── Summary cards ───────────────────────────────────────── */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <Card label="Total cost" value={fmtDollar(summary.totalCost)} />
          <Card label="LLM ticks" value={summary.totalTicks.toLocaleString()} />
          <Card label="Avg cost / tick" value={fmtDollar(summary.avgCostPerTick)} />
          <Card
            label="Cache hit rate"
            value={`${(summary.cacheHitRate * 100).toFixed(1)}%`}
            accent={summary.cacheHitRate > 0.5}
          />
        </div>

        {/* ── Row: ticks with/without cache breaks ────────────────── */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <Card
            label="Clean ticks"
            value={summary.ticksClean.toLocaleString()}
            sub="no panel broke"
          />
          <Card
            label="Break ticks"
            value={summary.ticksWithBreak.toLocaleString()}
            sub="cache broken"
          />
          <Card label="Hit tokens" value={fmtTokens(summary.totalHitTokens)} />
          <Card label="Miss tokens" value={fmtTokens(summary.totalMissTokens)} />
        </div>

        {/* ── Row: Culprit donut + Cost timeline ──────────────────── */}
        <div className="grid gap-6 lg:grid-cols-2">
          <Section>
            <DonutChart data={culprits} title="Cache break culprits" />
          </Section>
          <Section>
            <CostTimeline rows={filtered} title="Cost per tick over time" />
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

        {/* ── Row: Culprit cost attribution ────────────────────────── */}
        {culpritCosts.length > 0 && (
          <Section>
            <HBarChart data={culpritCosts} title="Top culprits by associated cost" />
          </Section>
        )}

        {/* ── Row: Max freeze per culprit ─────────────────────────── */}
        {maxFreezes.length > 0 && (
          <Section>
            <HBarChart data={maxFreezes} title="Max freezes per culprit" format={String} />
          </Section>
        )}

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
  return <div className="rounded-xl border border-border bg-card p-5 card-shadow">{children}</div>
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

const BREAK_KINDS: { v: string; text: string }[] = [
  { v: "all", text: "All" },
  { v: "no_break", text: "No break" },
  { v: "content_changed", text: "Changed" },
  { v: "panel_appeared", text: "Appeared" },
  { v: "panel_disappeared", text: "Disappeared" },
]

function BreakKindFilter({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex items-center gap-1.5">
      <span className="text-[11px] font-medium text-muted-foreground">Break</span>
      <div className="flex overflow-hidden rounded-lg border border-border">
        {BREAK_KINDS.map((o) => (
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
