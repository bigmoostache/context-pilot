import { useState } from "react"
import { Snowflake } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { TokenBar } from "@/components/panels/TokenBar"
import { panels, tokenBudget, cacheStats } from "@/lib/mock"
import { panelIcon, fmtTokens, fmtCost, loadColor } from "@/lib/panelMeta"
import { cn } from "@/lib/utils"

export function LeftRail() {
  const [selected, setSelected] = useState("P5")
  const usedRatio = tokenBudget.used / tokenBudget.budget

  return (
    <aside className="rise flex w-[290px] shrink-0 flex-col border-r border-border bg-[oklch(0.165_0.006_75)]">
      {/* budget meter */}
      <div className="border-b border-border px-3 py-2.5">
        <div className="mb-1.5 flex items-baseline justify-between">
          <span className="label">context budget</span>
          <span
            className="text-[11px] font-semibold tabular-nums"
            style={{ color: loadColor(usedRatio) }}
          >
            {(usedRatio * 100).toFixed(1)}%
          </span>
        </div>
        <TokenBar value={tokenBudget.used} max={tokenBudget.budget} className="h-1.5" />
        <div className="mt-1 flex justify-between text-[10px] tabular-nums text-muted-foreground">
          <span>{fmtTokens(tokenBudget.used)} used</span>
          <span className="text-[var(--warn)]/70">{fmtTokens(tokenBudget.threshold)} clean</span>
          <span>{fmtTokens(tokenBudget.budget)} max</span>
        </div>
      </div>

      {/* panel list */}
      <div className="flex items-center justify-between px-3 pt-2.5 pb-1">
        <span className="label">panels · {panels.length}</span>
        <span className="label">tokens</span>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <ul className="px-1.5 pb-2">
          {panels.map((p) => {
            const Icon = panelIcon[p.kind]
            const sel = selected === p.id
            return (
              <li key={p.id}>
                <button
                  type="button"
                  onClick={() => setSelected(p.id)}
                  className={cn(
                    "group flex w-full items-center gap-2 rounded-[3px] px-1.5 py-1 text-left transition-colors",
                    sel
                      ? "bg-[oklch(0.24_0.012_75)] ring-1 ring-[var(--signal)]/40"
                      : "hover:bg-[oklch(0.21_0.008_75)]",
                  )}
                >
                  <span
                    className={cn(
                      "size-1 shrink-0 rounded-full",
                      sel ? "bg-[var(--signal)] shadow-[0_0_5px_var(--signal)]" : "bg-[var(--grid)]",
                    )}
                  />
                  <Icon
                    className="size-3.5 shrink-0"
                    style={{ color: sel ? "var(--signal)" : "var(--muted-foreground)" }}
                  />
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="flex items-center gap-1">
                      <span className="truncate text-[11.5px] text-foreground/90">{p.name}</span>
                      <span className="text-[9px] text-muted-foreground/60">{p.id}</span>
                      {p.frozen != null && (
                        <Snowflake className="size-2.5 shrink-0 text-[var(--interactive)]/70" />
                      )}
                    </span>
                    <TokenBar
                      value={p.tokens}
                      max={panels[0].tokens}
                      animate={false}
                      className="h-[3px]"
                    />
                  </span>
                  <span className="flex shrink-0 flex-col items-end gap-0.5">
                    <span className="text-[10px] tabular-nums text-foreground/70">
                      {fmtTokens(p.tokens)}
                    </span>
                    <span
                      className={cn(
                        "text-[9px] tabular-nums",
                        p.misses > 3 ? "text-[var(--warn)]/80" : "text-muted-foreground/50",
                      )}
                    >
                      ×{p.misses}
                    </span>
                  </span>
                </button>
              </li>
            )
          })}
        </ul>
      </ScrollArea>

      {/* cache stats footer */}
      <div className="border-t border-border px-3 py-2">
        <span className="label">cache economics</span>
        <div className="mt-1.5 grid grid-cols-3 gap-y-1 text-[10px] tabular-nums">
          <Stat k="hit" v={fmtTokens(cacheStats.hit)} c="var(--ok)" />
          <Stat k="miss" v={fmtTokens(cacheStats.miss)} c="var(--warn)" />
          <Stat k="out" v={fmtTokens(cacheStats.out)} c="var(--interactive)" />
        </div>
        <div className="mt-1.5 flex items-center justify-between border-t border-border/60 pt-1.5 text-[10px]">
          <span className="text-muted-foreground">{cacheStats.uncached} uncached</span>
          <span className="font-semibold text-[var(--warn)]">{fmtCost(cacheStats.costUsd)}</span>
        </div>
      </div>
    </aside>
  )
}

function Stat({ k, v, c }: { k: string; v: string; c: string }) {
  return (
    <div className="flex flex-col">
      <span className="text-[9px] uppercase tracking-wider text-muted-foreground/60">{k}</span>
      <span style={{ color: c }} className="font-semibold">
        {v}
      </span>
    </div>
  )
}
