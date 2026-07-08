import { Gauge } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { useAgentMeta, useMetrics, usePanels } from "@/lib/live"
import { fmtTokens, fmtCost, loadColor } from "@/lib/support/panelMeta"
import { PanelFrame, PanelSection, InspectionUnavailable } from "../../PanelFrame"

/**
 * Statistics panel — session vitals, served from live backend state.
 *
 * Every figure here is a *truthful* read, never mock chrome:
 *   - **Session** rows show the agent's cumulative-since-boot token totals
 *     (`input`/`output`, folded from `CostAggregate` into the view and exposed
 *     on `/metrics`) and total spend (`/meta` `costUsd`).
 *   - **Panel context** is the exact sum of the live inspection-panel token
 *     weights the cockpit can see (`/panels`) — labelled as such, *not* as the
 *     agent's full context-window occupancy, which it deliberately is not.
 *   - **Context elements** lists each live panel's weight as a mini bar.
 *
 * Two figures the read-only web inspection plane structurally *cannot* serve —
 * the agent's live context-window occupancy (its private working-set size) and
 * the cache hit/miss token split (tracked in the agent, never journaled to the
 * oplog) — are surfaced as an honest unavailable notice rather than faked.
 */
export function StatsPanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: panels = [] } = usePanels(agentId)
  const { data: meta } = useAgentMeta(agentId)
  const { data: metrics } = useMetrics(agentId)

  // Panel context = exact sum of the inspection panels the cockpit can see.
  const panelTokens = panels.reduce((sum, p) => sum + p.tokens, 0)
  const maxTokens = panels.length > 0 ? Math.max(...panels.map((p) => p.tokens)) : 1
  // A sensible reference ceiling for the meter (the common model budget); the
  // bar reflects how heavy the *visible panels* are, not a hard agent limit.
  const REF_BUDGET = 200_000
  const usedRatio = Math.min(1, panelTokens / REF_BUDGET)

  const inTok = metrics?.tokens?.input ?? 0
  const outTok = metrics?.tokens?.output ?? 0
  const costUsd = meta?.costUsd ?? metrics?.breaker.spendUsd ?? 0

  return (
    <PanelFrame
      icon={Gauge}
      name="Statistics"
      subtitle="Session vitals"
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <PanelSection label="Session (cumulative since boot)">
        <div className="grid grid-cols-3 gap-2">
          <Stat label="Input" value={fmtTokens(inTok)} color="var(--interactive)" />
          <Stat label="Output" value={fmtTokens(outTok)} color="var(--ok)" />
          <Stat label="Cost" value={fmtCost(costUsd)} color="var(--signal)" />
        </div>
      </PanelSection>

      <PanelSection label="Panel context">
        <div className="mb-1.5 flex items-baseline justify-between">
          <span className="text-[12px] text-muted-foreground">
            {fmtTokens(panelTokens)} across {panels.length} panel{panels.length === 1 ? "" : "s"}
          </span>
          <span
            className="text-[12px] font-semibold tabular-nums"
            style={{ color: loadColor(usedRatio) }}
          >
            {(usedRatio * 100).toFixed(0)}%
          </span>
        </div>
        <div className="h-2 overflow-hidden rounded-full bg-muted">
          <div
            className="fill-sweep h-full rounded-full"
            style={{ width: `${usedRatio * 100}%`, background: loadColor(usedRatio) }}
          />
        </div>
      </PanelSection>

      <PanelSection label="Context elements">
        <ul className="flex flex-col gap-1">
          {panels.map((p) => (
            <li key={p.id} className="flex items-center gap-2">
              <span className="w-28 shrink-0 truncate text-[11.5px] text-foreground/80">
                {p.name}
              </span>
              <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full"
                  style={{
                    width: `${(p.tokens / maxTokens) * 100}%`,
                    background: "var(--signal-dim)",
                  }}
                />
              </div>
              <span className="w-12 shrink-0 text-right text-[10px] text-muted-foreground/70 tabular-nums">
                {fmtTokens(p.tokens)}
              </span>
            </li>
          ))}
        </ul>
      </PanelSection>

      <PanelSection label="Cache economics & live context size">
        <InspectionUnavailable reason="The cache hit/miss token split and the agent's live context-window occupancy are private working-set state — they are never journaled to the oplog, so the read-only web inspection plane cannot serve them. Surfacing them truthfully needs a dedicated agent-emitted metric." />
      </PanelSection>
    </PanelFrame>
  )
}

function Stat({ label, value, color }: { label: string; value: string; color: string }) {
  return (
    <div className="card-shadow rounded-lg border border-border bg-card p-2.5">
      <div className="text-[10px] tracking-wide text-muted-foreground/65 uppercase">{label}</div>
      <div className="mt-0.5 text-[13px] font-semibold tabular-nums" style={{ color }}>
        {value}
      </div>
    </div>
  )
}
