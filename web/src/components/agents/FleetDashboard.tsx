import { useState } from "react"
import { AlertTriangle, Bot, FolderGit2, FolderPlus, Settings2 } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { accentVar, fmtCost, FLEET_MAX_W } from "@/lib/support/panelMeta"
import { useMetrics, useAgentMeta } from "@/lib/live"
import { avatarUrl } from "@/lib/api"
import { clickable } from "@/lib/support/a11y"
import type { Agent, AgentStatus } from "@/lib/types"
import { cn } from "@/lib/utils"
import { AgentModal } from "./AgentModal"
import { RetiredSection } from "./FleetRetired"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs input", color: "var(--signal)" },
  idle: { label: "Standby", color: "var(--muted-foreground)" },
  disconnected: { label: "Disconnected", color: "var(--danger)" },
  waiting: { label: "Restarting", color: "var(--interactive)" },
}

type Modal = { mode: "create" } | { mode: "manage"; agent: Agent } | null

/** Shared row grid: name grows, then health · model · status · cost · action.
 *  One template used by both the header strip and every data row so the columns
 *  stay pixel-aligned (Linear projects-list convention). */
const ROW_GRID = "grid grid-cols-[minmax(0,1fr)_130px_170px_130px_84px_32px] items-center gap-3"

/**
 * Fleet welcome dashboard — mission control and the SOLE place agents are
 * managed. A Linear-style dense list: a sticky-feeling column header over one
 * row per agent (1 agent = 1 folder), plus the create / manage flows (the
 * per-agent views no longer touch agent management).
 */
export function FleetDashboard({
  agents,
  onOpenAgent,
}: {
  agents: Agent[]
  onOpenAgent: (id: string) => void
}) {
  const [modal, setModal] = useState<Modal>(null)
  const [toast, setToast] = useState<string | null>(null)

  const flash = (m: string) => {
    setToast(m)
    window.setTimeout(() => setToast(null), 2200)
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ScrollArea className="min-h-0 flex-1 bg-background">
        <div className={cn("mx-auto flex w-full flex-col gap-6 px-8 py-9", FLEET_MAX_W)}>
          <header className="flex items-end justify-between gap-4">
            <div className="flex items-center gap-2.5">
              <h1 className="text-[24px] font-semibold tracking-tight text-foreground">Agents</h1>
              <span className="rounded-full bg-muted/60 px-2 py-0.5 text-[11px] font-medium text-muted-foreground/70 tabular-nums">
                {agents.length}
              </span>
            </div>
            <button
              onClick={() => setModal({ mode: "create" })}
              className="flex shrink-0 items-center gap-2 rounded-lg bg-(--interactive) px-3.5 py-2 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105"
            >
              <FolderPlus className="size-4" />
              New agent
            </button>
          </header>

          {/* Active fleet — one bordered list card with a column header and a
              divider between every row (the Linear projects-list look). */}
          <div className="card-shadow overflow-hidden rounded-xl border border-border bg-card">
            <ColumnHeader />
            <div className="divide-y divide-border/60">
              {agents.map((a) => (
                <AgentRow
                  key={a.id}
                  agent={a}
                  onOpen={() => onOpenAgent(a.id)}
                  onManage={() => setModal({ mode: "manage", agent: a })}
                />
              ))}
              <NewAgentRow onClick={() => setModal({ mode: "create" })} />
            </div>
          </div>

          <RetiredSection onFlash={flash} />
        </div>
      </ScrollArea>

      {modal && <AgentModal modal={modal} onClose={() => setModal(null)} onFlash={flash} />}

      {toast && (
        <div className="pop-shadow absolute bottom-6 left-1/2 z-30 -translate-x-1/2 rounded-lg border border-border bg-card px-4 py-2 text-[12px] text-foreground/90">
          {toast}
        </div>
      )}
    </div>
  )
}

/** The muted uppercase column-label strip above the agent rows. Mirrors
 *  {@link ROW_GRID} so labels sit exactly over their cells. */
function ColumnHeader() {
  return (
    <div
      className={cn(
        ROW_GRID,
        "border-b border-border bg-muted/40 px-4 py-2 text-[10.5px] font-semibold tracking-wide text-muted-foreground/70 uppercase",
      )}
    >
      <span>Agent</span>
      <span>Health</span>
      <span>Model</span>
      <span>Status</span>
      <span className="text-right">Cost</span>
      <span />
    </div>
  )
}

function AgentRow({
  agent,
  onOpen,
  onManage,
}: {
  agent: Agent
  onOpen: () => void
  onManage: () => void
}) {
  // Live vitals (status dot, cost) ride the per-agent meta cache, which the SSE
  // bridge folds in real time (T297); the polled fleet row is only the fallback
  // until the first delta lands. ensureSync(agent.id) is triggered by this hook.
  const { data: live } = useAgentMeta(agent.id)
  const a = live ?? agent
  const s = statusMeta[a.status]
  const accent = accentVar[a.accent]

  return (
    // Whole row opens the agent (keyboard-activatable via clickable()); the
    // trailing gear manages it and stops propagation so it never also opens.
    <div
      {...clickable(onOpen)}
      className={cn(
        ROW_GRID,
        "group cursor-pointer px-4 py-2.5 transition-colors hover:bg-muted/40",
      )}
    >
      {/* Agent — avatar/icon + name over a dim one-line task summary. */}
      <div className="flex min-w-0 items-center gap-3">
        {a.hasAvatar ? (
          <img
            src={avatarUrl(agent.id)}
            alt={agent.name}
            className="size-8 shrink-0 rounded-lg object-cover"
          />
        ) : (
          <span
            className="flex size-8 shrink-0 items-center justify-center rounded-lg"
            style={{ background: `color-mix(in oklab, ${accent} 16%, transparent)`, color: accent }}
          >
            <FolderGit2 className="size-4" />
          </span>
        )}
        <div className="flex min-w-0 flex-col leading-tight">
          <span className="truncate text-[13.5px] font-semibold text-foreground/90">
            {agent.name}
          </span>
          {agent.task && (
            <span className="truncate text-[11px] text-muted-foreground/70">{agent.task}</span>
          )}
        </div>
      </div>

      {/* §19 health — a degraded stream / lagging projection surfaces here so it
          is VISIBLE, never a silent backend latch (T121). */}
      <div className="min-w-0">
        <HealthBadge agentId={agent.id} />
      </div>

      {/* Model */}
      <span className="inline-flex min-w-0 items-center gap-1.5 text-[11.5px] text-muted-foreground">
        <Bot className="size-3.5 shrink-0" />
        <span className="truncate">{agent.model}</span>
      </span>

      {/* Status pill */}
      <span
        className="inline-flex w-fit shrink-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[10.5px] font-medium"
        style={{ background: `color-mix(in oklab, ${s.color} 14%, transparent)`, color: s.color }}
      >
        <span
          className={cn("size-1.5 rounded-full", a.status === "working" && "animate-pulse")}
          style={{ background: s.color }}
        />
        {s.label}
      </span>

      {/* Cost */}
      <span className="text-right text-[12px] font-semibold text-foreground/80 tabular-nums">
        {fmtCost(a.costUsd)}
      </span>

      {/* Manage — appears on row hover; stops propagation so it doesn't open. */}
      <button
        onClick={(e) => {
          e.stopPropagation()
          onManage()
        }}
        title="Manage agent"
        className="flex size-7 items-center justify-center rounded-md text-muted-foreground/60 opacity-0 transition-[opacity,color,background-color] group-hover:opacity-100 hover:bg-muted hover:text-(--interactive) focus-visible:opacity-100"
      >
        <Settings2 className="size-4" />
      </button>
    </div>
  )
}

/** Rev-lag threshold above which the projection is flagged as falling behind.
 *  Under the live 5ms tail the lag is 0–1; a sustained lag this high means the
 *  backend view is no longer tracking the oplog head (a real health signal). */
const REV_LAG_WARN = 50

/** The first non-nominal health condition to surface for an agent row, or null
 *  when everything is nominal. A flat if-chain (not a nested ternary): a
 *  degraded stream first, then a lagging projection. */
function healthCondition(
  data: NonNullable<ReturnType<typeof useMetrics>["data"]>,
): { label: string; tone: string; title: string } | null {
  const { stream, rev } = data
  if (stream.degraded) {
    return {
      label: "Stream degraded",
      tone: "var(--warn)",
      title: `Live token stream dropped ${stream.droppedFrames} frame(s) — a slow consumer is being shed (the durable record is unaffected).`,
    }
  }
  if ((rev.lag ?? 0) > REV_LAG_WARN) {
    return {
      label: "Projection lagging",
      tone: "var(--warn)",
      title: `Backend view is ${rev.lag} revs behind the oplog head (view ${rev.view} / head ${rev.oplogHead ?? "?"}). The projection is falling behind the durable log.`,
    }
  }
  return null
}

/**
 * §19 health badge for an agent row. Polls `/api/agent/{id}/metrics` and
 * surfaces the *first* non-nominal condition as a coloured pill — so a degraded
 * stream or a lagging projection is **visible at a glance** on the fleet board
 * rather than a silent backend latch (T121). When everything is nominal (or
 * metrics haven't loaded) it renders a dim dash, keeping the column aligned.
 */
function HealthBadge({ agentId }: { agentId: string }) {
  const { data } = useMetrics(agentId)
  const condition = data ? healthCondition(data) : null
  if (!condition) return <span className="text-[12px] text-muted-foreground/40">—</span>

  return (
    <span
      role="status"
      title={condition.title}
      className="inline-flex w-fit items-center gap-1.5 rounded-md px-2 py-0.5 text-[10.5px] font-medium"
      style={{
        background: `color-mix(in oklab, ${condition.tone} 14%, transparent)`,
        color: condition.tone,
      }}
    >
      <AlertTriangle className="size-3" />
      {condition.label}
    </span>
  )
}

/** The trailing dashed "create agent" row that closes the list (Linear's
 *  add-row affordance). Spans the full width rather than sitting in the grid. */
function NewAgentRow({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="flex w-full items-center gap-2.5 px-4 py-2.5 text-[12.5px] font-medium text-muted-foreground/70 transition-colors hover:bg-muted/40 hover:text-(--interactive)"
    >
      <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-dashed border-border">
        <FolderPlus className="size-4" />
      </span>
      New agent
      <span className="truncate text-[11px] font-normal text-muted-foreground/50">
        Initialize an agent in a folder — its realm for the whole session.
      </span>
    </button>
  )
}
