import { useState } from "react"
import { AlertTriangle, Bot, FolderGit2, Plus, Settings2 } from "lucide-react"
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
  "needs-you": { label: "Needs input", color: "var(--signal)" },
  working: { label: "Working", color: "var(--interactive)" },
  waiting: { label: "Restarting", color: "var(--interactive)" },
  idle: { label: "Standby", color: "var(--muted-foreground)" },
  disconnected: { label: "Disconnected", color: "var(--danger)" },
}

/** Group render order — the most actionable state first (agents needing input),
 *  then live/restarting, then dormant, then offline (mirrors ThreadList's
 *  turn-first grouping). Groups with no members are skipped. */
const GROUP_ORDER: AgentStatus[] = ["needs-you", "working", "waiting", "idle", "disconnected"]

type Modal = { mode: "create" } | { mode: "manage"; agent: Agent } | null

/**
 * Fleet dashboard — mission control and the SOLE place agents are managed.
 *
 * Agents are grouped by status into labelled sections (the Linear projects-list
 * idiom, and the same grouping the thread sidebar uses), each a run of soft
 * hover-cards. One agent = one folder = one row; the whole row opens it, the
 * hover gear manages it. Live status/cost fold in over the SSE bridge (T297).
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

  const openRow = (a: Agent) => (
    <AgentRow
      key={a.id}
      agent={a}
      onOpen={() => onOpenAgent(a.id)}
      onManage={() => setModal({ mode: "manage", agent: a })}
    />
  )

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ScrollArea className="min-h-0 flex-1 bg-background">
        <div className={cn("mx-auto flex w-full flex-col p-6", FLEET_MAX_W)}>
          {/* Understated toolbar: title + count on the left, a small New action
              on the right (mirrors the thread sidebar header). */}
          <header className="flex h-8 items-center justify-between px-1">
            <div className="flex items-center gap-2">
              <h1 className="text-[15px] font-semibold text-foreground">Agents</h1>
              <span className="text-[12px] text-muted-foreground/60 tabular-nums">
                {agents.length}
              </span>
            </div>
            <button
              onClick={() => setModal({ mode: "create" })}
              className="flex items-center gap-1.5 rounded-md bg-(--interactive) px-2.5 py-1.5 text-[12px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105"
            >
              <Plus className="size-3.5" />
              New agent
            </button>
          </header>

          <div className="mt-2">
            {GROUP_ORDER.map((status) => {
              const members = agents.filter((a) => a.status === status)
              if (members.length === 0) return null
              return (
                <section key={status} className="mb-1">
                  <GroupHeader status={status} count={members.length} />
                  {members.map((a) => openRow(a))}
                </section>
              )
            })}
            <NewAgentRow onClick={() => setModal({ mode: "create" })} />
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

/** A status section's header: a coloured dot + the status label + member count,
 *  in the thread-sidebar group-header type scale. */
function GroupHeader({ status, count }: { status: AgentStatus; count: number }) {
  const s = statusMeta[status]
  return (
    <div className="flex items-center gap-2 px-2.5 pt-3 pb-1.5 first:pt-1">
      <span className="size-1.5 rounded-full" style={{ background: s.color }} />
      <span className="text-[11px] font-semibold text-muted-foreground">{s.label}</span>
      <span className="text-[11px] text-muted-foreground/45 tabular-nums">{count}</span>
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
  // Live vitals (status, cost) ride the per-agent meta cache the SSE bridge folds
  // in real time (T297); the polled fleet row is the fallback until the first
  // delta lands. ensureSync(agent.id) is triggered by this hook.
  const { data: live } = useAgentMeta(agent.id)
  const a = live ?? agent
  const accent = accentVar[a.accent]

  return (
    // Whole row opens the agent (keyboard-activatable via clickable()); the gear
    // manages it and stops propagation so it never also opens. Soft hover-card,
    // like a thread row.
    <div
      {...clickable(onOpen)}
      className="group hover:card-shadow relative mb-0.5 flex cursor-pointer items-center gap-3 rounded-lg px-2.5 py-2 transition-colors select-none hover:bg-card"
    >
      {/* Identity — avatar (or accent folder chip) + name + a dim one-line task
          summary trailing on the same line so the row stays single-height. */}
      {a.hasAvatar ? (
        <img
          src={avatarUrl(agent.id)}
          alt={agent.name}
          className="size-6 shrink-0 rounded-md object-cover"
        />
      ) : (
        <span
          className="flex size-6 shrink-0 items-center justify-center rounded-md"
          style={{ background: `color-mix(in oklab, ${accent} 16%, transparent)`, color: accent }}
        >
          <FolderGit2 className="size-3.5" />
        </span>
      )}
      <span className="shrink-0 truncate text-[13px] font-medium text-foreground/90">
        {agent.name}
      </span>
      {agent.task && (
        <span className="min-w-0 flex-1 truncate text-[11.5px] text-muted-foreground/55">
          {agent.task}
        </span>
      )}
      <span className={agent.task ? "shrink-0" : "flex-1"} />

      {/* Health — only when non-nominal (§19, T121): a degraded stream / lagging
          projection surfaces as a small badge, else nothing. */}
      <HealthBadge agentId={agent.id} />

      {/* Model — plain muted token. */}
      <span className="hidden shrink-0 items-center gap-1.5 text-[11.5px] text-muted-foreground/70 sm:inline-flex">
        <Bot className="size-3.5 text-muted-foreground/45" />
        {agent.model}
      </span>

      {/* Cost — tabular, muted. */}
      <span className="shrink-0 text-[11.5px] text-muted-foreground/70 tabular-nums">
        {fmtCost(a.costUsd)}
      </span>

      {/* Manage — reveals on row hover; stops propagation so it doesn't open.
          Kept in flow (reserved width) so the row doesn't reflow on hover. */}
      <button
        onClick={(e) => {
          e.stopPropagation()
          onManage()
        }}
        title="Manage agent"
        className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/50 opacity-0 transition-[opacity,color,background-color] group-hover:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100"
      >
        <Settings2 className="size-3.5" />
      </button>
    </div>
  )
}

/** Rev-lag threshold above which the projection is flagged as falling behind.
 *  Under the live 5ms tail the lag is 0–1; a sustained lag this high means the
 *  backend view is no longer tracking the oplog head (a real health signal). */
const REV_LAG_WARN = 50

/** The first non-nominal health condition for an agent row, or null when
 *  everything is nominal. A flat if-chain (not a nested ternary): a degraded
 *  stream first, then a lagging projection. */
function healthCondition(
  data: NonNullable<ReturnType<typeof useMetrics>["data"]>,
): { label: string; title: string } | null {
  const { stream, rev } = data
  if (stream.degraded) {
    return {
      label: "degraded",
      title: `Live token stream dropped ${stream.droppedFrames} frame(s) — a slow consumer is being shed (the durable record is unaffected).`,
    }
  }
  if ((rev.lag ?? 0) > REV_LAG_WARN) {
    return {
      label: "lagging",
      title: `Backend view is ${rev.lag} revs behind the oplog head (view ${rev.view} / head ${rev.oplogHead ?? "?"}). The projection is falling behind the durable log.`,
    }
  }
  return null
}

/**
 * §19 health badge for an agent row. Polls `/api/agent/{id}/metrics` and, only
 * when something is off, surfaces the first non-nominal condition as a small
 * amber uppercase pill (the thread-row badge idiom) so a degraded stream or a
 * lagging projection stays visible at a glance rather than a silent backend
 * latch (T121). Nominal renders nothing — a healthy fleet stays uncluttered.
 */
function HealthBadge({ agentId }: { agentId: string }) {
  const { data } = useMetrics(agentId)
  const condition = data ? healthCondition(data) : null
  if (!condition) return null

  return (
    <span
      role="status"
      title={condition.title}
      className="inline-flex shrink-0 items-center gap-1 rounded-full px-1.5 py-px text-[9.5px] font-semibold tracking-wide uppercase"
      style={{
        background: "color-mix(in oklab, var(--warn) 18%, transparent)",
        color: "var(--warn)",
      }}
    >
      <AlertTriangle className="size-2.5" />
      {condition.label}
    </span>
  )
}

/** The trailing "new agent" affordance — a soft hover-card row matching the
 *  agent rows, with a dashed chip. */
function NewAgentRow({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="group hover:card-shadow mt-1 flex w-full items-center gap-3 rounded-lg px-2.5 py-2 text-left transition-colors hover:bg-card"
    >
      <span className="flex size-6 shrink-0 items-center justify-center rounded-md border border-dashed border-border text-muted-foreground/60 group-hover:text-(--interactive)">
        <Plus className="size-3.5" />
      </span>
      <span className="text-[13px] font-medium text-muted-foreground/70 group-hover:text-foreground">
        New agent
      </span>
      <span className="truncate text-[11.5px] text-muted-foreground/45">
        Initialize an agent in a folder — its realm for the whole session.
      </span>
    </button>
  )
}
