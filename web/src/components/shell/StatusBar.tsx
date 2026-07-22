import { Boxes, Bot, ChevronsUpDown, Loader2, MessagesSquare, RefreshCw, Wallet } from "lucide-react"
import { fmtCost, fmtTokens } from "@/lib/support/panelMeta"
import { useLibrary, sendCommand } from "@/lib/live"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
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
  activeAgentId = "",
  connected = true,
  onRestart,
  restarting = false,
  loading = false,
}: {
  fleet?: boolean
  agents?: Agent[]
  activeAgent?: Agent | undefined
  /** The focused agent's id — drives the behaviour selector's library query + command. */
  activeAgentId?: string
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
      agentId={activeAgentId}
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

/** Pull the footer's numeric vitals off the agent with safe fallbacks, so the
 *  host component's branch count stays under the complexity budget. */
function agentVitals(agent?: Agent) {
  return {
    costUsd: agent?.costUsd ?? 0,
    used: agent?.contextUsed ?? 0,
    budget: agent?.contextBudget ?? 200_000,
    threshold: agent?.contextThreshold ?? 0,
    hit: agent?.contextHit ?? 0,
    miss: agent?.contextMiss ?? 0,
  }
}

/** Single-agent session vitals — shown while an agent is focused. */
function AgentStatus({
  agent,
  agentId,
  connected = true,
  onRestart,
  restarting = false,
  loading = false,
}: {
  agent?: Agent | undefined
  agentId?: string
  connected?: boolean
  onRestart?: (() => void) | undefined
  restarting?: boolean
  loading?: boolean
}) {
  // Use the LIVE execution phase folded from the PhaseTransition delta (T297)
  // so the footer distinguishes streaming · tooling · ready instead of the old
  // 2-state projection of `status`. Falls back to `status` only before the
  // first phase transition has been observed (phase still undefined).
  const phase: StreamPhase = resolvePhase(agent)
  const p = phaseMeta[phase]
  // Context-window meter figures — the agent's OWN authoritative occupancy +
  // cache split, folded from the ContextUsage push delta (T297), byte-identical
  // to the ratatui sidebar.
  const { costUsd, used, budget, threshold, hit, miss } = agentVitals(agent)

  return (
    <footer className="vibrancy flex h-8 shrink-0 items-center gap-3 border-t border-border px-4 text-[12px]">
      <StatusIndicator
        phase={p}
        connected={connected}
        restarting={restarting}
        loading={loading}
        onRestart={onRestart}
      />

      {agentId ? <BehaviourChip agentId={agentId} /> : null}

      <span className="ml-auto flex items-center gap-3">
        <ContextBar used={used} threshold={threshold} budget={budget} hit={hit} miss={miss} />
        <span className="h-3.5 w-px bg-border" />
        <span className="text-muted-foreground tabular-nums">{fmtCost(costUsd)}</span>
      </span>
    </footer>
  )
}

/**
 * Active-behaviour-agent chip + selector — right of the "Ready" indicator.
 *
 * Shows the loaded system-prompt agent's name (caveman / default / worker …)
 * and, on click, a dropdown of the agent's prompt-library behaviour agents.
 * Picking one issues a `load_behaviour` command down the SAME live path threads
 * use (`sendCommand → POST /command → apply_command → set_active_agent`), so
 * this component holds zero business logic (M141): the active flag comes from
 * the backend's `library()` inspect read, the switch is a fire-and-forget
 * command whose effect lands back through the library query on the next poll.
 */
function BehaviourChip({ agentId }: { agentId: string }) {
  const { data: library = [] } = useLibrary(agentId)
  const agentBehaviours = library.filter((item) => item.kind === "agent")
  const active = agentBehaviours.find((item) => item.active)
  const activeName = active?.name ?? "default"

  const select = (id: string) => {
    if (id === active?.id) return
    void sendCommand(agentId, { kind: "load_behaviour", id }).catch(() => {
      // Fire-and-forget: a failed switch keeps the current behaviour; the
      // library query re-reports ground truth on its next poll.
    })
  }

  return (
    <>
      <span className="h-3.5 w-px bg-border" />
      <DropdownMenu>
        <DropdownMenuTrigger className="flex cursor-pointer items-center gap-1.5 rounded-sm px-1.5 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground/85 focus:outline-none">
          <Bot className="size-3.5" />
          <span className="max-w-[120px] truncate font-medium text-foreground/80">{activeName}</span>
          <ChevronsUpDown className="size-3 opacity-60" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" side="top" className="min-w-44">
          <DropdownMenuLabel>System prompt</DropdownMenuLabel>
          <DropdownMenuSeparator />
          {agentBehaviours.length === 0 ? (
            <DropdownMenuItem disabled>No behaviours</DropdownMenuItem>
          ) : (
            agentBehaviours.map((item) => (
              <DropdownMenuItem
                key={item.id}
                onClick={() => select(item.id)}
                className={item.active ? "font-semibold text-foreground" : ""}
              >
                <span className="flex items-center gap-2">
                  {item.active ? (
                    <span className="size-1.5 rounded-full bg-(--ok)" />
                  ) : (
                    <span className="size-1.5" />
                  )}
                  {item.name}
                </span>
              </DropdownMenuItem>
            ))
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  )
}

/** The footer's left indicator: a loading / restarting / connected phase dot, or
 *  a clickable "Disconnected" restart button when the push plane is down.
 *  Extracted from {@link AgentStatus} so its host stays under the complexity
 *  budget — this is where the four-way state cascade lives. */
function StatusIndicator({
  phase,
  connected,
  restarting,
  loading,
  onRestart,
}: {
  phase: { label: string; color: string }
  connected: boolean
  restarting: boolean
  loading: boolean
  onRestart?: (() => void) | undefined
}) {
  if (loading) {
    return (
      <span className="flex items-center gap-1.5">
        <Loader2 className="size-3.5 animate-spin text-muted-foreground" />
        <span className="text-muted-foreground">Loading…</span>
      </span>
    )
  }
  if (restarting) {
    return (
      <span className="flex items-center gap-1.5">
        <RefreshCw className="size-3.5 animate-spin text-muted-foreground" />
        <span className="font-medium text-muted-foreground">Restarting…</span>
      </span>
    )
  }
  if (connected) {
    return (
      <span className="flex items-center gap-1.5">
        <span className="size-2 rounded-full" style={{ background: phase.color }} />
        <span className="font-medium text-foreground/80">{phase.label}</span>
      </span>
    )
  }
  return (
    <button
      type="button"
      onClick={onRestart}
      className="flex cursor-pointer items-center gap-1.5 rounded-sm px-1 py-0.5 transition-colors hover:bg-muted"
    >
      <span className="size-2 rounded-full bg-(--danger)" />
      <span className="font-medium text-(--danger)">Disconnected</span>
    </button>
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
