import { Gauge } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { stats, tokenBudget, cacheStats } from "@/lib/mock"
import { usePanels } from "@/lib/live"
import { fmtTokens, fmtCost, loadColor } from "@/lib/panelMeta"
import { PanelFrame, PanelSection } from "./PanelFrame"

const ACCENT: Record<string, string> = {
  signal: "var(--signal)",
  interactive: "var(--interactive)",
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
}

/**
 * Statistics panel — session vitals. A context-budget meter heads the
 * panel, followed by the headline stat rows and a compact "context elements"
 * table showing each live panel's token weight as a mini bar.
 */
export function StatsPanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: panels = [] } = usePanels(agentId)
  const usedRatio = tokenBudget.used / tokenBudget.budget
  const maxTokens = panels.length > 0 ? Math.max(...panels.map((p) => p.tokens)) : 1

  return (
    <PanelFrame
      icon={Gauge}
      name="Statistics"
      subtitle="Session vitals"
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <PanelSection label="Context budget">
        <div className="mb-1.5 flex items-baseline justify-between">
          <span className="text-[12px] text-muted-foreground">
            {fmtTokens(tokenBudget.used)} of {fmtTokens(tokenBudget.budget)}
          </span>
          <span className="text-[12px] font-semibold tabular-nums" style={{ color: loadColor(usedRatio) }}>
            {(usedRatio * 100).toFixed(0)}%
          </span>
        </div>
        <div className="h-2 overflow-hidden rounded-full bg-muted">
          <div
            className="h-full rounded-full fill-sweep"
            style={{ width: `${usedRatio * 100}%`, background: loadColor(usedRatio) }}
          />
        </div>
      </PanelSection>

      <PanelSection label="Session">
        <div className="grid grid-cols-2 gap-2">
          {stats.map((s) => (
            <div key={s.label} className="rounded-lg border border-border bg-card p-2.5 card-shadow">
              <div className="text-[10px] uppercase tracking-wide text-muted-foreground/65">{s.label}</div>
              <div
                className="mt-0.5 text-[13px] font-semibold tabular-nums"
                style={{ color: s.accent ? ACCENT[s.accent] : "var(--foreground)" }}
              >
                {s.value}
              </div>
            </div>
          ))}
        </div>
      </PanelSection>

      <PanelSection label="Cache economics">
        <div className="flex gap-2 text-[11px]">
          <CacheChip label="hit" value={fmtTokens(cacheStats.hit)} color="var(--ok)" />
          <CacheChip label="miss" value={fmtTokens(cacheStats.miss)} color="var(--warn)" />
          <CacheChip label="out" value={fmtTokens(cacheStats.out)} color="var(--interactive)" />
          <CacheChip label="cost" value={fmtCost(cacheStats.costUsd)} color="var(--signal)" />
        </div>
      </PanelSection>

      <PanelSection label="Context elements">
        <ul className="flex flex-col gap-1">
          {panels.map((p) => (
            <li key={p.id} className="flex items-center gap-2">
              <span className="w-28 shrink-0 truncate text-[11.5px] text-foreground/80">{p.name}</span>
              <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full"
                  style={{ width: `${(p.tokens / maxTokens) * 100}%`, background: "var(--signal-dim)" }}
                />
              </div>
              <span className="w-12 shrink-0 text-right text-[10px] tabular-nums text-muted-foreground/70">
                {fmtTokens(p.tokens)}
              </span>
            </li>
          ))}
        </ul>
      </PanelSection>
    </PanelFrame>
  )
}

function CacheChip({ label, value, color }: { label: string; value: string; color: string }) {
  return (
    <div className="flex-1 rounded-lg border border-border bg-card px-2 py-1.5 text-center card-shadow">
      <div className="text-[9px] uppercase tracking-wide text-muted-foreground/60">{label}</div>
      <div className="text-[12px] font-semibold tabular-nums" style={{ color }}>
        {value}
      </div>
    </div>
  )
}
