import { useMemo, useState } from "react"
import { Coins, Hash, ServerCrash } from "lucide-react"
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
import { useFleet, useFleetMetrics } from "@/lib/live"
import type { Agent, UsageUnit } from "@/lib/types"
import type { AgentMetrics } from "@/lib/api"
import { cn } from "@/lib/utils"

/**
 * Cost & token usage — the fleet "Usage" page, served from **live** backend
 * state (no mock, no fabricated history).
 *
 * The backend retains, per agent, only what the oplog durably carries:
 *   - cumulative-since-boot **spend** (the durable cost-breaker high-water),
 *   - cumulative **input / output token** totals (folded from `CostAggregate`),
 *   - the agent's spend **budget** and breaker trip state.
 *
 * It does **not** retain a month-by-month time series, nor the cache hit/miss
 * token split (that split is private agent working-set state, never journaled).
 * So this page is honest about its grain: it shows the live *current totals*
 * per agent and fleet-wide, with a unit lens ($ / tokens) and an optional
 * single-agent filter — and an explicit notice that historical bucketing and
 * the hit/miss breakdown are not available over the read-only inspection plane.
 */

/** One agent's live usage row, joined from fleet meta + §19 metrics. */
interface Row {
  agent: Agent
  spendUsd: number
  budgetUsd: number
  inputTokens: number
  outputTokens: number
  tripped: boolean
}

const agentAccent = (a: Agent) =>
  ({
    signal: "var(--signal)",
    interactive: "var(--interactive)",
    ok: "var(--ok)",
    warn: "var(--warn)",
    danger: "var(--danger)",
  })[a.accent]

export function UsagePage() {
  const [unit, setUnit] = useState<UsageUnit>("usd")
  const [agentId, setAgentId] = useState<string>("all")

  const { data: agents = [] } = useFleet()
  const { data: metrics = [] } = useFleetMetrics()

  // Join the two live sources by agent id into one usage row each.
  const rows = useMemo<Row[]>(() => {
    const byId = new Map<string, AgentMetrics>(metrics.map((m) => [m.id, m]))
    return agents.map((agent) => {
      const m = byId.get(agent.id)
      return {
        agent,
        // Prefer the durable breaker high-water; fall back to meta costUsd.
        spendUsd: m?.breaker.spendUsd ?? agent.costUsd,
        budgetUsd: m?.breaker.budgetUsd ?? 0,
        inputTokens: m?.tokens?.input ?? 0,
        outputTokens: m?.tokens?.output ?? 0,
        tripped: m?.breaker.tripped ?? false,
      }
    })
  }, [agents, metrics])

  const visible = agentId === "all" ? rows : rows.filter((r) => r.agent.id === agentId)

  const totals = useMemo(
    () =>
      visible.reduce(
        (acc, r) => ({
          spendUsd: acc.spendUsd + r.spendUsd,
          inputTokens: acc.inputTokens + r.inputTokens,
          outputTokens: acc.outputTokens + r.outputTokens,
        }),
        { spendUsd: 0, inputTokens: 0, outputTokens: 0 },
      ),
    [visible],
  )

  const fmtUsd = (v: number) =>
    `$${v < 10 ? v.toFixed(2) : v < 1000 ? v.toFixed(0) : `${(v / 1000).toFixed(1)}K`}`
  const fmtTok = (v: number) =>
    v >= 1e6 ? `${(v / 1e6).toFixed(2)}M` : v >= 1e3 ? `${(v / 1e3).toFixed(0)}K` : `${Math.round(v)}`
  const fmt = unit === "usd" ? fmtUsd : fmtTok
  // The active-unit fleet total (drives the sr-only live announcement).
  const totalValue = unit === "usd" ? totals.spendUsd : totals.inputTokens + totals.outputTokens

  return (
    <ScrollArea className="h-full">
      <div className="mx-auto flex max-w-[920px] flex-col gap-5 p-6">
        {/* header + controls */}
        <header className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-col gap-0.5">
            <h2 className="text-[17px] font-semibold text-foreground">Usage</h2>
            <p className="text-[12.5px] text-muted-foreground">
              Live cumulative-since-boot totals across the fleet
            </p>
          </div>
          <div className="flex items-center gap-2">
            {/* agent filter */}
            <select
              value={agentId}
              onChange={(e) => setAgentId(e.target.value)}
              className="rounded-lg border border-border bg-card px-2.5 py-1.5 text-[12.5px] text-foreground/85 card-shadow"
              aria-label="Filter by agent"
            >
              <option value="all">All agents</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            {/* unit lens */}
            <div className="flex items-center rounded-lg border border-border bg-muted/60 p-0.5">
              <UnitTab active={unit === "usd"} onClick={() => setUnit("usd")} icon={Coins} label="$" />
              <UnitTab active={unit === "tokens"} onClick={() => setUnit("tokens")} icon={Hash} label="Tokens" />
            </div>
          </div>
        </header>

        {/* summary cards */}
        <section className="grid grid-cols-3 gap-3">
          <SummaryCard label="Total spend" value={fmtUsd(totals.spendUsd)} accent="var(--signal)" />
          <SummaryCard label="Input tokens" value={fmtTok(totals.inputTokens)} accent="var(--interactive)" />
          <SummaryCard label="Output tokens" value={fmtTok(totals.outputTokens)} accent="var(--ok)" />
        </section>

        {/* per-agent table */}
        <section className="flex flex-col gap-2.5">
          <span className="text-[13px] font-semibold text-foreground/90">By agent</span>
          {visible.length === 0 ? (
            <div className="rounded-xl border border-border bg-card p-8 text-center text-[12.5px] text-muted-foreground card-shadow">
              No agents in the fleet yet.
            </div>
          ) : (
            <div className="overflow-hidden rounded-xl border border-border bg-card card-shadow">
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead>Agent</TableHead>
                    <TableHead className="text-right">Input</TableHead>
                    <TableHead className="text-right">Output</TableHead>
                    <TableHead className="text-right">Spend</TableHead>
                    <TableHead className="text-right">{unit === "usd" ? "% budget" : "Total"}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {visible.map((r) => {
                    const pct = r.budgetUsd > 0 ? (r.spendUsd / r.budgetUsd) * 100 : null
                    return (
                      <TableRow key={r.agent.id}>
                        <TableCell className="font-medium text-foreground/85">
                          <span className="flex items-center gap-2">
                            <span className="size-2 shrink-0 rounded-full" style={{ background: agentAccent(r.agent) }} />
                            {r.agent.name}
                            {r.tripped && (
                              <span className="rounded-full bg-[var(--danger)]/14 px-1.5 py-px text-[9.5px] font-medium text-[var(--danger)]">
                                over budget
                              </span>
                            )}
                          </span>
                        </TableCell>
                        <TableCell className="text-right tabular-nums text-foreground/75">{fmtTok(r.inputTokens)}</TableCell>
                        <TableCell className="text-right tabular-nums text-foreground/75">{fmtTok(r.outputTokens)}</TableCell>
                        <TableCell className="text-right font-semibold tabular-nums text-foreground/90">{fmtUsd(r.spendUsd)}</TableCell>
                        <TableCell className="text-right tabular-nums text-muted-foreground/80">
                          {unit === "usd" ? (pct != null ? `${pct.toFixed(0)}%` : "—") : fmtTok(r.inputTokens + r.outputTokens)}
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
                <TableFooter>
                  <TableRow className="hover:bg-transparent">
                    <TableCell className="font-semibold text-foreground/90">Total</TableCell>
                    <TableCell className="text-right font-semibold tabular-nums text-foreground/85">{fmtTok(totals.inputTokens)}</TableCell>
                    <TableCell className="text-right font-semibold tabular-nums text-foreground/85">{fmtTok(totals.outputTokens)}</TableCell>
                    <TableCell className="text-right font-semibold tabular-nums text-foreground">{fmtUsd(totals.spendUsd)}</TableCell>
                    <TableCell className="text-right font-semibold tabular-nums text-foreground/85">
                      {unit === "usd" ? "" : fmtTok(totals.inputTokens + totals.outputTokens)}
                    </TableCell>
                  </TableRow>
                </TableFooter>
              </Table>
            </div>
          )}
          <p className="sr-only">Active unit total: {fmt(totalValue)}</p>
        </section>

        {/* honest boundary notice */}
        <section
          role="note"
          className="flex items-start gap-2.5 rounded-xl border border-dashed border-border bg-muted/30 p-4"
        >
          <ServerCrash className="mt-0.5 size-4 shrink-0 text-muted-foreground/70" />
          <p className="text-[12px] leading-relaxed text-muted-foreground">
            A month-by-month usage history and the cache <strong>hit/miss</strong> token split are
            not shown: the backend retains only cumulative-since-boot totals on the oplog, and the
            hit/miss split is private agent working-set state that is never journaled. Surfacing
            either truthfully would need a dedicated agent-emitted usage time series.
          </p>
        </section>
      </div>
    </ScrollArea>
  )
}

function SummaryCard({ label, value, accent }: { label: string; value: string; accent: string }) {
  return (
    <div className="flex flex-col gap-1 rounded-xl border border-border bg-card p-4 card-shadow">
      <span className="text-[11px] uppercase tracking-wide text-muted-foreground/65">{label}</span>
      <span className="text-[20px] font-semibold tabular-nums" style={{ color: accent }}>
        {value}
      </span>
    </div>
  )
}

function UnitTab({
  active,
  onClick,
  icon: Icon,
  label,
}: {
  active: boolean
  onClick: () => void
  icon: typeof Coins
  label: string
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12px] font-medium transition-colors",
        active ? "bg-card text-foreground card-shadow" : "text-muted-foreground hover:text-foreground/80",
      )}
    >
      <Icon className="size-3.5" />
      {label}
    </button>
  )
}
