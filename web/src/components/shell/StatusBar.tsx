import { Boxes, MessagesSquare, Wallet } from "lucide-react"
import { usePanels } from "@/lib/live"
import { fmtCost, fmtTokens } from "@/lib/support/panelMeta"
import type { Agent, StreamPhase } from "@/lib/types"

const phaseMeta: Record<StreamPhase, { label: string; color: string }> = {
  ready: { label: "Ready", color: "var(--ok)" },
  streaming: { label: "Streaming", color: "var(--signal)" },
  tooling: { label: "Working", color: "var(--interactive)" },
  blocked: { label: "Blocked", color: "var(--danger)" },
}

/**
 * Bottom status footer. Its contents are altitude-aware:
 *
 * - **Inside an agent** (`fleet=false`): live session vitals for the focused
 *   agent — stream phase, agent name, and its running cost.
 * - **At fleet altitude** (`fleet=true`, no agent selected): per-agent vitals
 *   are meaningless, so we show fleet-wide aggregates instead — how many agents
 *   you run, how many threads are in flight across all of them, and the *total*
 *   spend (clearly labelled). A "Needs you" count surfaces how many agents are
 *   waiting on input, so the footer doubles as a glanceable fleet pulse.
 */
export function StatusBar({ fleet = false, agents = [], activeAgent }: { fleet?: boolean; agents?: Agent[]; activeAgent?: Agent }) {
  return fleet ? <FleetStatus agents={agents} /> : <AgentStatus agent={activeAgent} />
}

/** Fleet-wide aggregates — shown when no single agent is focused. */
function FleetStatus({ agents }: { agents: Agent[] }) {
  const totalSpend = agents.reduce((sum, a) => sum + a.costUsd, 0)
  const needsYou = agents.filter((a) => a.status === "needs-you").length
  const totalThreads = agents.reduce((sum, a) => sum + a.threads, 0)

  return (
    <footer className="vibrancy flex h-8 shrink-0 items-center gap-4 border-t border-border px-4 text-[12px]">
      <span className="font-medium text-foreground/70">Fleet</span>
      <span className="h-3.5 w-px bg-border" />

      <Metric icon={Boxes} label="Agents" value={String(agents.length)} />
      <Metric icon={MessagesSquare} label="Threads" value={String(totalThreads)} />

      {needsYou > 0 && (
        <span className="flex items-center gap-1.5 text-muted-foreground">
          <span className="size-2 rounded-full" style={{ background: "var(--signal)" }} />
          <span className="tabular-nums text-foreground/80">{needsYou}</span>
          <span>need{needsYou === 1 ? "s" : ""} you</span>
        </span>
      )}

      <span className="ml-auto flex items-center gap-1.5 text-muted-foreground">
        <Wallet className="size-3.5" />
        <span>Total spend</span>
        <span className="tabular-nums font-medium text-foreground/85">{fmtCost(totalSpend)}</span>
      </span>
    </footer>
  )
}

/** Single-agent session vitals — shown while an agent is focused. */
function AgentStatus({ agent }: { agent?: Agent }) {
  // Use the LIVE execution phase folded from the PhaseTransition delta (T297)
  // so the footer distinguishes streaming · tooling · ready instead of the old
  // 2-state projection of `status`. Falls back to `status` only before the
  // first phase transition has been observed (phase still undefined).
  const phase: StreamPhase =
    agent?.phase === "streaming"
      ? "streaming"
      : agent?.phase === "tooling"
        ? "tooling"
        : agent?.phase === "idle"
          ? "ready"
          : agent?.status === "working"
            ? "streaming"
            : "ready"
  const p = phaseMeta[phase]
  const costUsd = agent?.costUsd ?? 0

  // Live context window stats from real panel data
  const { data: panels = [] } = usePanels(agent?.id ?? "")
  const budget = 200_000
  const totalTokens = panels.reduce((s, p) => s + (p.tokens ?? 0), 0)
  // Panels that have never missed a cache cycle are "hits"
  const hitTokens = panels
    .filter((p) => p.cached)
    .reduce((s, p) => s + (p.tokens ?? 0), 0)
  const missTokens = totalTokens - hitTokens

  return (
    <footer className="vibrancy flex h-8 shrink-0 items-center gap-3 border-t border-border px-4 text-[12px]">
      <span className="flex items-center gap-1.5">
        <span className="size-2 rounded-full" style={{ background: p.color }} />
        <span className="font-medium text-foreground/80">{p.label}</span>
      </span>

      <span className="ml-auto flex items-center gap-3">
        <ContextBar hit={hitTokens} miss={missTokens} budget={budget} />
        <span className="h-3.5 w-px bg-border" />
        <span className="tabular-nums text-muted-foreground">{fmtCost(costUsd)}</span>
      </span>
    </footer>
  )
}

/**
 * Fixed-width context-window meter. The bar is the whole context budget; the
 * filled portion is split into **cache hits** (green — already-cached, cheap
 * tokens) and **misses** (yellow — fresh input that had to be sent), with the
 * remaining **free** space shown in grey. Hovering reveals an "ultra-nice"
 * tooltip above the bar breaking down the exact token counts.
 */
function ContextBar({ hit, miss, budget }: { hit: number; miss: number; budget: number }) {
  const free = Math.max(0, budget - hit - miss)
  const pct = (v: number) => `${(v / budget) * 100}%`
  const used = hit + miss

  return (
    <div className="group/cb relative flex items-center">
      {/* the meter */}
      <div className="flex h-2 w-28 overflow-hidden rounded-full bg-muted ring-1 ring-border/60">
        <span style={{ width: pct(hit), background: "var(--ok)" }} className="h-full" />
        <span style={{ width: pct(miss), background: "var(--warn)" }} className="h-full" />
      </div>

      {/* tooltip — opens upward, above the bar */}
      <div className="pointer-events-none absolute bottom-full left-1/2 z-50 mb-2 -translate-x-1/2 translate-y-1 opacity-0 transition-all duration-150 group-hover/cb:translate-y-0 group-hover/cb:opacity-100">
        <div className="w-[188px] rounded-lg border border-border bg-popover p-2.5 pop-shadow">
          <div className="mb-2 flex items-baseline justify-between">
            <span className="text-[11px] font-semibold text-foreground/90">Context window</span>
            <span className="font-mono text-[10px] tabular-nums text-muted-foreground">
              {((used / budget) * 100).toFixed(0)}%
            </span>
          </div>
          <TipRow color="var(--ok)" label="Cache hits" value={fmtTokens(hit)} />
          <TipRow color="var(--warn)" label="Misses" value={fmtTokens(miss)} />
          <TipRow color="var(--muted-foreground)" label="Free" value={fmtTokens(free)} dim />
          <div className="mt-2 flex items-center justify-between border-t border-border/60 pt-1.5">
            <span className="text-[10.5px] text-muted-foreground">Used / budget</span>
            <span className="font-mono text-[10.5px] font-medium tabular-nums text-foreground/85">
              {fmtTokens(used)} / {fmtTokens(budget)}
            </span>
          </div>
        </div>
        {/* caret */}
        <div className="absolute left-1/2 top-full size-2 -translate-x-1/2 -translate-y-1 rotate-45 border-b border-r border-border bg-popover" />
      </div>
    </div>
  )
}

/** One token-breakdown line inside the context tooltip. */
function TipRow({
  color,
  label,
  value,
  dim,
}: {
  color: string
  label: string
  value: string
  dim?: boolean
}) {
  return (
    <div className="flex items-center gap-2 py-0.5">
      <span className="size-2 shrink-0 rounded-[3px]" style={{ background: color }} />
      <span className={`text-[11px] ${dim ? "text-muted-foreground" : "text-foreground/80"}`}>{label}</span>
      <span className="ml-auto font-mono text-[10.5px] tabular-nums text-foreground/85">{value}</span>
    </div>
  )
}

/** A small icon · label · value triple used by the fleet footer. */
function Metric({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof Boxes
  label: string
  value: string
}) {
  return (
    <span className="flex items-center gap-1.5 text-muted-foreground">
      <Icon className="size-3.5" />
      <span className="tabular-nums font-medium text-foreground/85">{value}</span>
      <span>{label}</span>
    </span>
  )
}
