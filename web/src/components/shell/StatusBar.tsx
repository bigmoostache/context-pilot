import { Boxes, Loader2, MessagesSquare, RefreshCw, Wallet } from "lucide-react"
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
export function StatusBar({
  fleet = false,
  agents = [],
  activeAgent,
  connected = true,
  onRestart,
  restarting = false,
  loading = false,
}: {
  fleet?: boolean
  agents?: Agent[]
  activeAgent?: Agent | undefined
  /** False when the SSE push plane for this agent is down. */
  connected?: boolean
  /** Fires the restart API + full reconnect lifecycle (same as AgentModal Restart). */
  onRestart?: () => void
  /** True while the restart lifecycle is in-flight (API call through SSE reconnect). */
  restarting?: boolean
  /** True while the agent meta is loading after a switch (show loader, not stale phase). */
  loading?: boolean
}) {
  return fleet ? (
    <FleetStatus agents={agents} />
  ) : (
    <AgentStatus
      agent={activeAgent}
      connected={connected}
      onRestart={onRestart}
      restarting={restarting}
      loading={loading}
    />
  )
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
          <span className="text-foreground/80 tabular-nums">{needsYou}</span>
          <span>need{needsYou === 1 ? "s" : ""} you</span>
        </span>
      )}

      <span className="ml-auto flex items-center gap-1.5 text-muted-foreground">
        <Wallet className="size-3.5" />
        <span>Total spend</span>
        <span className="font-medium text-foreground/85 tabular-nums">{fmtCost(totalSpend)}</span>
      </span>
    </footer>
  )
}

/** Resolve the live execution phase from the agent's folded vitals. Prefers the
 *  push-delta `phase` (streaming/tooling/idle), falling back to `status` before
 *  the first PhaseTransition has been observed. A flat if-chain, not a nested
 *  ternary. */
function resolvePhase(agent?: Agent): StreamPhase {
  if (agent?.phase === "streaming") return "streaming"
  if (agent?.phase === "tooling") return "tooling"
  if (agent?.phase === "idle") return "ready"
  if (agent?.status === "working") return "streaming"
  return "ready"
}

/** Single-agent session vitals — shown while an agent is focused. */
function AgentStatus({
  agent,
  connected = true,
  onRestart,
  restarting = false,
  loading = false,
}: {
  agent?: Agent | undefined
  connected?: boolean
  onRestart?: () => void
  restarting?: boolean
  loading?: boolean
}) {
  // Use the LIVE execution phase folded from the PhaseTransition delta (T297)
  // so the footer distinguishes streaming · tooling · ready instead of the old
  // 2-state projection of `status`. Falls back to `status` only before the
  // first phase transition has been observed (phase still undefined).
  const phase: StreamPhase = resolvePhase(agent)
  const p = phaseMeta[phase]
  const costUsd = agent?.costUsd ?? 0

  // Context-window meter — the agent's OWN authoritative occupancy, folded from
  // the ContextUsage push delta (T297), so this is byte-identical to the
  // ratatui sidebar's `used / threshold / budget` line, not a frontend re-sum.
  const used = agent?.contextUsed ?? 0
  const budget = agent?.contextBudget ?? 200_000
  const threshold = agent?.contextThreshold ?? 0
  // Cache hit/miss split of `used` (hit + miss === used), folded from the same
  // ContextUsage delta — lets the meter draw ratatui's green/amber segments.
  const hit = agent?.contextHit ?? 0
  const miss = agent?.contextMiss ?? 0

  return (
    <footer className="vibrancy flex h-8 shrink-0 items-center gap-3 border-t border-border px-4 text-[12px]">
      {loading ? (
        <span className="flex items-center gap-1.5">
          <Loader2 className="size-3.5 animate-spin text-muted-foreground" />
          <span className="text-muted-foreground">Loading…</span>
        </span>
      ) : restarting ? (
        <span className="flex items-center gap-1.5">
          <RefreshCw className="size-3.5 animate-spin text-muted-foreground" />
          <span className="font-medium text-muted-foreground">Restarting…</span>
        </span>
      ) : connected ? (
        <span className="flex items-center gap-1.5">
          <span className="size-2 rounded-full" style={{ background: p.color }} />
          <span className="font-medium text-foreground/80">{p.label}</span>
        </span>
      ) : (
        <button
          type="button"
          onClick={onRestart}
          className="flex cursor-pointer items-center gap-1.5 rounded px-1 py-0.5 transition-colors hover:bg-muted"
        >
          <span className="size-2 rounded-full bg-[var(--danger)]" />
          <span className="font-medium text-[var(--danger)]">Disconnected</span>
        </button>
      )}

      <span className="ml-auto flex items-center gap-3">
        <ContextBar used={used} threshold={threshold} budget={budget} hit={hit} miss={miss} />
        <span className="h-3.5 w-px bg-border" />
        <span className="text-muted-foreground tabular-nums">{fmtCost(costUsd)}</span>
      </span>
    </footer>
  )
}

/**
 * Fixed-width context-window meter — the agent's authoritative occupancy.
 *
 * The bar is the whole context budget; the filled portion is `used` tokens,
 * drawn (when the agent reports a cache split) as ratatui's two segments — a
 * green **hit** run followed by an amber **miss** run — so the web meter mirrors
 * the TUI sidebar token bar exactly (T297). When no split has arrived yet
 * (`hit + miss === 0`) it falls back to a single fill coloured by proximity to
 * the cleaning threshold. A thin tick marks the threshold; hovering reveals the
 * exact `Used (hit)` / `Used (miss)` / Threshold / Free figures — the SAME
 * numbers the agent renders in its ratatui sidebar.
 */
function ContextBar({
  used,
  threshold,
  budget,
  hit,
  miss,
}: {
  used: number
  threshold: number
  budget: number
  hit: number
  miss: number
}) {
  const safeBudget = budget > 0 ? budget : 200_000
  const usedRatio = Math.min(1, used / safeBudget)
  const thresholdRatio = threshold > 0 ? Math.min(1, threshold / safeBudget) : 0
  // Has the agent reported a hit/miss split? (cold/older agents send only used)
  const hasSplit = hit + miss > 0
  const hitRatio = Math.min(1, hit / safeBudget)
  const missRatio = Math.min(1, miss / safeBudget)
  // Single-fill fallback colour by proximity to the threshold.
  const overThreshold = threshold > 0 && used >= threshold
  const nearThreshold = threshold > 0 && used >= threshold * 0.85
  const fallbackFill = overThreshold ? "var(--danger)" : nearThreshold ? "var(--warn)" : "var(--ok)"
  const free = Math.max(0, safeBudget - used)

  return (
    <div className="group/cb relative flex items-center">
      {/* the meter */}
      <div className="relative h-2 w-28 overflow-hidden rounded-full bg-muted ring-1 ring-border/60">
        {hasSplit ? (
          // ratatui's two-segment bar: green hit run + amber miss run.
          <div className="flex h-full">
            <span
              style={{ width: `${hitRatio * 100}%`, background: "var(--ok)" }}
              className="block h-full transition-[width] duration-1000 ease-out"
            />
            <span
              style={{ width: `${missRatio * 100}%`, background: "var(--warn)" }}
              className="block h-full transition-[width] duration-1000 ease-out"
            />
          </div>
        ) : (
          <span
            style={{ width: `${usedRatio * 100}%`, background: fallbackFill }}
            className="block h-full rounded-full transition-[width] duration-1000 ease-out"
          />
        )}
        {/* threshold tick */}
        {thresholdRatio > 0 && thresholdRatio < 1 && (
          <span
            className="absolute top-0 h-full w-px bg-foreground/50 transition-[left] duration-1000 ease-out"
            style={{ left: `${thresholdRatio * 100}%` }}
          />
        )}
      </div>

      {/* tooltip — opens upward, above the bar */}
      <div className="pointer-events-none absolute bottom-full left-1/2 z-50 mb-2 -translate-x-1/2 translate-y-1 opacity-0 transition-all duration-150 group-hover/cb:translate-y-0 group-hover/cb:opacity-100">
        <div className="pop-shadow w-[188px] rounded-lg border border-border bg-popover p-2.5">
          <div className="mb-2 flex items-baseline justify-between">
            <span className="text-[11px] font-semibold text-foreground/90">Context window</span>
            <span className="font-mono text-[10px] text-muted-foreground tabular-nums">
              {(usedRatio * 100).toFixed(0)}%
            </span>
          </div>
          {hasSplit ? (
            <>
              <TipRow color="var(--ok)" label="Used (hit)" value={fmtTokens(hit)} />
              <TipRow color="var(--warn)" label="Used (miss)" value={fmtTokens(miss)} />
            </>
          ) : (
            <TipRow color={fallbackFill} label="Used" value={fmtTokens(used)} />
          )}
          {threshold > 0 && (
            <TipRow color="var(--foreground)" label="Threshold" value={fmtTokens(threshold)} />
          )}
          <TipRow color="var(--muted-foreground)" label="Free" value={fmtTokens(free)} dim />
          <div className="mt-2 flex items-center justify-between gap-8 border-t border-border/60 pt-1.5">
            <span className="shrink-0 text-[10.5px] text-muted-foreground">Used / budget</span>
            <span className="font-mono text-[10.5px] font-medium text-foreground/85 tabular-nums">
              {fmtTokens(used)} / {fmtTokens(safeBudget)}
            </span>
          </div>
        </div>
        {/* caret */}
        <div className="absolute top-full left-1/2 size-2 -translate-x-1/2 -translate-y-1 rotate-45 border-r border-b border-border bg-popover" />
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
      <span className={`text-[11px] ${dim ? "text-muted-foreground" : "text-foreground/80"}`}>
        {label}
      </span>
      <span className="ml-auto font-mono text-[10.5px] text-foreground/85 tabular-nums">
        {value}
      </span>
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
      <span className="font-medium text-foreground/85 tabular-nums">{value}</span>
      <span>{label}</span>
    </span>
  )
}
