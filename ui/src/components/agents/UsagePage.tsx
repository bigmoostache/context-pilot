import { useState } from "react"
import { Coins, Cpu, Layers, MessageSquare } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { usage } from "@/lib/mock"
import { accentVar, fmtCost, fmtTokens } from "@/lib/panelMeta"
import { cn } from "@/lib/utils"

/**
 * Usage & cost analytics — the fleet dashboard's "Usage" page. A read-only,
 * nicely-formatted view to investigate spend and token consumption, per agent
 * or in aggregate. Design-only: all figures come from {@link usage} mock data;
 * the charts are CSS-only (no charting dependency).
 */
export function UsagePage() {
  const { rows, spend, cache } = usage
  const [range, setRange] = useState("7d")

  const totalCost = rows.reduce((a, r) => a + r.costUsd, 0)
  const totalIn = rows.reduce((a, r) => a + r.inputTokens, 0)
  const totalOut = rows.reduce((a, r) => a + r.outputTokens, 0)
  const totalCache = rows.reduce((a, r) => a + r.cacheTokens, 0)
  const totalTokens = totalIn + totalOut + totalCache
  const totalMsgs = rows.reduce((a, r) => a + r.messages, 0)
  const hitRate = Math.round((cache.hit / (cache.hit + cache.miss)) * 100)
  const maxCost = Math.max(...rows.map((r) => r.costUsd))

  return (
    <ScrollArea className="min-h-0 flex-1 bg-background">
      <div className="mx-auto flex w-full max-w-[920px] flex-col gap-7 px-8 py-9">
        {/* header */}
        <header className="flex items-end justify-between gap-4">
          <div className="flex flex-col gap-1.5">
            <span className="label">Analytics</span>
            <h1 className="text-[24px] font-semibold tracking-tight text-foreground">Usage &amp; cost</h1>
            <p className="text-[13px] text-muted-foreground">
              Inspect spend and token consumption across the fleet — per agent or in aggregate.
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-0.5 rounded-lg border border-border bg-muted/60 p-0.5">
            {["24h", "7d", "30d", "All"].map((r) => (
              <button
                key={r}
                onClick={() => setRange(r)}
                className={cn(
                  "rounded-md px-2.5 py-1 text-[12px] font-medium transition-all",
                  range === r ? "bg-card text-foreground card-shadow" : "text-muted-foreground hover:text-foreground",
                )}
              >
                {r}
              </button>
            ))}
          </div>
        </header>

        {/* hero stat cards */}
        <div className="grid grid-cols-2 gap-3.5 md:grid-cols-4">
          <HeroStat icon={Coins} label="Total spend" value={fmtCost(totalCost)} sub={`${range} window`} accent="var(--signal)" />
          <HeroStat icon={Cpu} label="Tokens" value={fmtTokens(totalTokens)} sub={`${fmtTokens(totalOut)} generated`} accent="var(--interactive)" />
          <HeroStat icon={Layers} label="Cache hit-rate" value={`${hitRate}%`} sub={`${fmtTokens(cache.hit)} reused`} accent="var(--ok)" />
          <HeroStat icon={MessageSquare} label="Messages" value={`${totalMsgs}`} sub={`${rows.length} agents`} accent="var(--warn)" />
        </div>

        {/* spend over time */}
        <Section title="Spend over time" hint={`${spend.length} most-recent slices`}>
          <Sparkline data={spend} />
        </Section>

        {/* per-agent breakdown */}
        <Section title="By agent" hint="Where the budget is going">
          <div className="flex flex-col gap-2.5">
            {[...rows]
              .sort((a, b) => b.costUsd - a.costUsd)
              .map((r) => {
                const accent = accentVar[r.accent]
                const pct = Math.round((r.costUsd / maxCost) * 100)
                return (
                  <div key={r.agentId} className="flex flex-col gap-2 rounded-xl border border-border bg-card px-4 py-3 card-shadow">
                    <div className="flex items-center gap-2.5">
                      <span className="size-2.5 shrink-0 rounded-full" style={{ background: accent }} />
                      <span className="text-[13.5px] font-semibold text-foreground/90">{r.agent}</span>
                      <span className="ml-auto font-mono text-[13px] font-semibold tabular-nums text-foreground">
                        {fmtCost(r.costUsd)}
                      </span>
                    </div>
                    {/* proportional cost bar */}
                    <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
                      <div
                        className="fill-sweep h-full rounded-full"
                        style={{ width: `${pct}%`, background: accent }}
                      />
                    </div>
                    {/* token breakdown */}
                    <div className="flex flex-wrap items-center gap-x-5 gap-y-1 text-[11px] text-muted-foreground">
                      <TokenStat label="Input" value={r.inputTokens} />
                      <TokenStat label="Output" value={r.outputTokens} />
                      <TokenStat label="Cache" value={r.cacheTokens} />
                      <span className="ml-auto tabular-nums">{r.messages} messages</span>
                    </div>
                  </div>
                )
              })}
          </div>
        </Section>

        {/* token composition + cache economics */}
        <div className="grid grid-cols-1 gap-3.5 md:grid-cols-2">
          <Section title="Token composition" hint="Whole session">
            <StackedBar
              parts={[
                { label: "Cache read", value: totalCache, color: "var(--ok)" },
                { label: "Input", value: totalIn, color: "var(--interactive)" },
                { label: "Output", value: totalOut, color: "var(--signal)" },
              ]}
            />
          </Section>

          <Section title="Cache economics" hint="Anthropic prompt cache">
            <div className="flex flex-col gap-2 rounded-xl border border-border bg-card px-4 py-3.5 card-shadow">
              <CacheRow label="Hits (reused)" value={fmtTokens(cache.hit)} color="var(--ok)" />
              <CacheRow label="Misses (recomputed)" value={fmtTokens(cache.miss)} color="var(--warn)" />
              <CacheRow label="Writes (cached)" value={fmtTokens(cache.write)} color="var(--interactive)" />
              <div className="mt-1 flex items-center justify-between border-t border-border/50 pt-2">
                <span className="text-[12px] font-medium text-foreground/80">Cache spend</span>
                <span className="font-mono text-[12.5px] font-semibold tabular-nums text-foreground">{fmtCost(cache.costUsd)}</span>
              </div>
            </div>
          </Section>
        </div>

        <p className="text-center text-[11px] text-muted-foreground/55">
          Design-only — figures are illustrative mock data.
        </p>
      </div>
    </ScrollArea>
  )
}

// ── pieces ────────────────────────────────────────────────────────

function HeroStat({
  icon: Icon,
  label,
  value,
  sub,
  accent,
}: {
  icon: typeof Coins
  label: string
  value: string
  sub: string
  accent: string
}) {
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-border bg-card px-4 py-3.5 card-shadow">
      <div className="flex items-center gap-2">
        <span
          className="flex size-7 items-center justify-center rounded-lg"
          style={{ background: `color-mix(in oklab, ${accent} 15%, transparent)`, color: accent }}
        >
          <Icon className="size-4" />
        </span>
        <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      </div>
      <span className="font-mono text-[22px] font-semibold leading-none tracking-tight tabular-nums text-foreground">
        {value}
      </span>
      <span className="text-[11px] text-muted-foreground/70">{sub}</span>
    </div>
  )
}

function Section({
  title,
  hint,
  children,
}: {
  title: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-baseline gap-2">
        <h2 className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">{title}</h2>
        {hint && <span className="text-[11px] text-muted-foreground/55">{hint}</span>}
      </div>
      {children}
    </section>
  )
}

function Sparkline({ data }: { data: number[] }) {
  const max = Math.max(...data)
  return (
    <div className="flex h-28 items-end gap-1.5 rounded-xl border border-border bg-card px-4 py-3 card-shadow">
      {data.map((v, i) => {
        const h = Math.max(4, Math.round((v / max) * 100))
        const last = i === data.length - 1
        return (
          <div key={i} className="group relative flex flex-1 items-end" title={fmtCost(v)}>
            <div
              className="fill-sweep w-full rounded-t-sm transition-colors"
              style={{
                height: `${h}%`,
                transformOrigin: "bottom",
                background: last ? "var(--signal)" : "color-mix(in oklab, var(--signal) 38%, transparent)",
              }}
            />
          </div>
        )
      })}
    </div>
  )
}

function TokenStat({ label, value }: { label: string; value: number }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className="text-muted-foreground/60">{label}</span>
      <span className="font-mono tabular-nums text-foreground/80">{fmtTokens(value)}</span>
    </span>
  )
}

function StackedBar({ parts }: { parts: { label: string; value: number; color: string }[] }) {
  const total = parts.reduce((a, p) => a + p.value, 0)
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-border bg-card px-4 py-3.5 card-shadow">
      <div className="flex h-3 w-full overflow-hidden rounded-full bg-muted">
        {parts.map((p) => (
          <div
            key={p.label}
            className="h-full first:rounded-l-full last:rounded-r-full"
            style={{ width: `${(p.value / total) * 100}%`, background: p.color }}
            title={`${p.label}: ${fmtTokens(p.value)}`}
          />
        ))}
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1.5">
        {parts.map((p) => (
          <span key={p.label} className="inline-flex items-center gap-1.5 text-[11px]">
            <span className="size-2 rounded-sm" style={{ background: p.color }} />
            <span className="text-muted-foreground">{p.label}</span>
            <span className="font-mono tabular-nums text-foreground/80">{fmtTokens(p.value)}</span>
          </span>
        ))}
      </div>
    </div>
  )
}

function CacheRow({ label, value, color }: { label: string; value: string; color: string }) {
  return (
    <div className="flex items-center gap-2.5">
      <span className="size-2 shrink-0 rounded-full" style={{ background: color }} />
      <span className="text-[12px] text-muted-foreground">{label}</span>
      <span className="ml-auto font-mono text-[12px] tabular-nums text-foreground/85">{value}</span>
    </div>
  )
}
