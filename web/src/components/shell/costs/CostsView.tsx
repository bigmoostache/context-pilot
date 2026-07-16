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
export function CostsView({
  agentId,
  disconnected,
  onReconnect,
}: {
  agentId: string
  disconnected?: boolean
  onReconnect?: () => void
}) {
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

  const blurStyle: { filter: string; transition: string } = disconnected
    ? { filter: "blur(3px) grayscale(0.5)", transition: "filter 300ms" }
    : { filter: "none", transition: "filter 300ms" }

  if (isLoading) {
    return (
      <CostsLoading disconnected={disconnected} onReconnect={onReconnect} blurStyle={blurStyle} />
    )
  }

  if (error || rows.length === 0) {
    return (
      <CostsEmpty disconnected={disconnected} onReconnect={onReconnect} blurStyle={blurStyle} />
    )
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-y-auto" style={blurStyle}>
      {disconnected && (
        <button
          onClick={onReconnect}
          className="absolute inset-0 z-40 cursor-pointer bg-background/30"
          aria-label="Reconnect to agent"
        />
      )}
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 p-6">
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

        {/* ── Summary cards (spend + cache efficiency + break split) ─ */}
        <SummaryCards summary={summary} />

        <ChartsArea
          filtered={filtered}
          culprits={culprits}
          costs={costs}
          tools={tools}
          culpritCosts={culpritCosts}
          maxFreezes={maxFreezes}
          crossTab={crossTab}
        />
      </div>
    </div>
  )
}

/** All chart sections below the summary cards. Extracted so {@link CostsView}
 *  stays under the P8 complexity(15) budget (was 17). */
function ChartsArea({
  filtered,
  culprits,
  costs,
  tools,
  culpritCosts,
  maxFreezes,
  crossTab,
}: {
  filtered: ReturnType<typeof parseCostTsv>
  culprits: ReturnType<typeof culpritDistribution>
  costs: ReturnType<typeof costBreakdown>
  tools: ReturnType<typeof toolCostAttribution>
  culpritCosts: ReturnType<typeof culpritCostAttribution>
  maxFreezes: ReturnType<typeof maxFreezePerCulprit>
  crossTab: ReturnType<typeof crossTabToolCulprit>
}) {
  return (
    <>
      <div className="grid gap-6 lg:grid-cols-2">
        <Section>
          <DonutChart data={culprits} title="Cache break culprits" />
        </Section>
        <Section>
          <CostTimeline rows={filtered} title="Cost per tick over time" />
        </Section>
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        <Section>
          <HBarChart data={costs} title="Cost by category" />
        </Section>
        <Section>
          <HBarChart data={tools} title="Top tools by associated cost" />
        </Section>
      </div>

      {culpritCosts.length > 0 && (
        <Section>
          <HBarChart data={culpritCosts} title="Top culprits by associated cost" />
        </Section>
      )}

      {maxFreezes.length > 0 && (
        <Section>
          <HBarChart data={maxFreezes} title="Max freezes per culprit" format={String} />
        </Section>
      )}

      {filtered.length > 0 && <TokenDistribution rows={filtered} />}

      {filtered.length > 0 && <ApiTokenDistribution rows={filtered} />}

      {crossTab.tools.length > 0 && crossTab.culprits.length > 0 && (
        <CrossTabTable crossTab={crossTab} totalTicks={filtered.length} />
      )}
    </>
  )
}

// ── Shared micro-components ─────────────────────────────────────────────────

/** Transparent overlay that captures clicks to reconnect — an accessible
 *  `<button>` instead of an unlabelled `<div>` with onClick. */
function DisconnectOverlay({ onReconnect }: { onReconnect?: (() => void) | undefined }) {
  if (!onReconnect) return null
  return (
    <button
      onClick={onReconnect}
      className="absolute inset-0 z-40 cursor-pointer bg-background/30"
      aria-label="Reconnect to agent"
    />
  )
}

/** Loading state — shown while the cost TSV is being fetched. */
function CostsLoading({
  disconnected,
  onReconnect,
  blurStyle,
}: {
  disconnected?: boolean | undefined
  onReconnect?: (() => void) | undefined
  blurStyle: { filter?: string; transition: string }
}) {
  return (
    <div
      className="relative flex min-h-0 flex-1 items-center justify-center text-muted-foreground"
      style={blurStyle}
    >
      <DisconnectOverlay onReconnect={disconnected ? onReconnect : undefined} />
      <span className="text-[13px]">Loading cost data…</span>
    </div>
  )
}

/** Empty / error state — shown when there is no cost data yet. */
function CostsEmpty({
  disconnected,
  onReconnect,
  blurStyle,
}: {
  disconnected?: boolean | undefined
  onReconnect?: (() => void) | undefined
  blurStyle: { filter?: string; transition: string }
}) {
  return (
    <div
      className="relative flex min-h-0 flex-1 flex-col items-center justify-center gap-2 text-muted-foreground"
      style={blurStyle}
    >
      <DisconnectOverlay onReconnect={disconnected ? onReconnect : undefined} />
      <span className="text-[15px] font-semibold">No cost data yet</span>
      <span className="max-w-sm text-center text-[12px]">
        Cost telemetry appears after the agent completes its first LLM tick. The file{" "}
        <code className="rounded-sm bg-muted px-1 py-0.5 text-[11px]">
          .context-pilot/logs/cost-tracking.tsv
        </code>{" "}
        will be created automatically.
      </span>
    </div>
  )
}

// ── Micro-components ────────────────────────────────────────────────────────

/** The two summary-card rows (spend + cache efficiency, then the break split).
 *  Extracted so {@link CostsView}'s render body stays within the P8 line budget. */
function SummaryCards({ summary }: { summary: ReturnType<typeof computeSummary> }) {
  return (
    <>
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
    </>
  )
}

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
    <div className="card-shadow flex flex-col gap-1 rounded-xl border border-border bg-card px-4 py-3">
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      <span
        className="text-[18px] font-bold tracking-tight tabular-nums"
        style={accent ? { color: "var(--ok, #4ade80)" } : undefined}
      >
        {value}
      </span>
      {sub && <span className="text-[10px] text-muted-foreground/70">{sub}</span>}
    </div>
  )
}

function Section({ children }: { children: React.ReactNode }) {
  return <div className="card-shadow rounded-xl border border-border bg-card p-5">{children}</div>
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
