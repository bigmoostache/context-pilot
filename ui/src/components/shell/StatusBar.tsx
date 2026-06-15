import { Boxes, MessagesSquare, Wallet } from "lucide-react"
import { status, agents, threadDetails } from "@/lib/mock"
import { fmtCost } from "@/lib/panelMeta"
import type { StreamPhase } from "@/lib/types"

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
export function StatusBar({ fleet = false }: { fleet?: boolean }) {
  return fleet ? <FleetStatus /> : <AgentStatus />
}

/** Fleet-wide aggregates — shown when no single agent is focused. */
function FleetStatus() {
  const totalSpend = agents.reduce((sum, a) => sum + a.costUsd, 0)
  const needsYou = agents.filter((a) => a.status === "needs-you").length

  return (
    <footer className="vibrancy flex h-8 shrink-0 items-center gap-4 border-t border-border px-4 text-[12px]">
      <span className="font-medium text-foreground/70">Fleet</span>
      <span className="h-3.5 w-px bg-border" />

      <Metric icon={Boxes} label="Agents" value={String(agents.length)} />
      <Metric icon={MessagesSquare} label="Threads" value={String(threadDetails.length)} />

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
function AgentStatus() {
  const p = phaseMeta[status.phase]
  return (
    <footer className="vibrancy flex h-8 shrink-0 items-center gap-3 border-t border-border px-4 text-[12px]">
      <span className="flex items-center gap-1.5">
        <span className="size-2 rounded-full" style={{ background: p.color }} />
        <span className="font-medium text-foreground/80">{p.label}</span>
      </span>

      <span className="h-3.5 w-px bg-border" />

      <span className="text-muted-foreground">{status.agent}</span>

      <span className="ml-auto tabular-nums text-muted-foreground">{fmtCost(status.costUsd)}</span>
    </footer>
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
