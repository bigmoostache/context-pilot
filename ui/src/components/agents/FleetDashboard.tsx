import {
  Bot,
  Clock,
  FolderGit2,
  FolderPlus,
  GitBranch,
  MessagesSquare,
  Rocket,
  Settings2,
} from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { agents, threadDetails } from "@/lib/mock"
import { accentVar, fmtCost } from "@/lib/panelMeta"
import type { Agent, AgentStatus } from "@/lib/types"
import { cn } from "@/lib/utils"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs you", color: "var(--signal)" },
  idle: { label: "Idle", color: "var(--muted-foreground)" },
}

/** Thread tally for an agent's realm — single source of truth = threadDetails. */
function realmStats(agentId: string) {
  const threads = threadDetails.filter((t) => t.agentId === agentId)
  return {
    total: threads.length,
    working: threads.filter((t) => t.status === "THEIR_TURN").length,
    waiting: threads.filter((t) => t.status === "MY_TURN").length,
  }
}

/**
 * Fleet welcome dashboard — the landing surface of the Agents view. A calm
 * "mission control" overview of every agent (1 agent = 1 folder): aggregate
 * stats up top, then a card per agent with its thread activity, plus an entry
 * point to create a new agent by browsing the filesystem.
 */
export function FleetDashboard({
  onOpenAgent,
  onManageAgent,
  onNewAgent,
}: {
  onOpenAgent: (id: string) => void
  onManageAgent: (id: string) => void
  onNewAgent: () => void
}) {
  // Aggregate fleet stats from the per-realm thread tallies.
  const totals = agents.reduce(
    (acc, a) => {
      const s = realmStats(a.id)
      acc.threads += s.total
      acc.working += s.working
      acc.waiting += s.waiting
      acc.cost += a.costUsd
      return acc
    },
    { threads: 0, working: 0, waiting: 0, cost: 0 },
  )

  return (
    <ScrollArea className="min-h-0 flex-1 bg-background">
      <div className="mx-auto flex w-full max-w-[940px] flex-col gap-7 px-8 py-9">
        {/* greeting */}
        <header className="flex items-end justify-between gap-4">
          <div className="flex flex-col gap-1.5">
            <span className="label">Mission control</span>
            <h1 className="text-[24px] font-semibold tracking-tight text-foreground">
              Your agents
            </h1>
            <p className="text-[13px] text-muted-foreground">
              Each agent lives in one folder — its realm. Switch in, or start a new one.
            </p>
          </div>
          <button
            onClick={onNewAgent}
            className="flex shrink-0 items-center gap-2 rounded-lg bg-[var(--interactive)] px-3.5 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
          >
            <FolderPlus className="size-4" />
            New agent
          </button>
        </header>

        {/* aggregate stat strip */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <Stat label="Agents" value={`${agents.length}`} icon={FolderGit2} />
          <Stat label="Threads" value={`${totals.threads}`} icon={MessagesSquare} />
          <Stat
            label="Working"
            value={`${totals.working}`}
            icon={Rocket}
            color="var(--interactive)"
            live={totals.working > 0}
          />
          <Stat
            label="Waiting on you"
            value={`${totals.waiting}`}
            icon={Clock}
            color="var(--signal)"
          />
        </div>

        {/* agent cards */}
        <div className="grid grid-cols-1 gap-3.5 md:grid-cols-2">
          {agents.map((a) => (
            <AgentCard
              key={a.id}
              agent={a}
              onOpen={() => onOpenAgent(a.id)}
              onManage={() => onManageAgent(a.id)}
            />
          ))}
          <NewAgentCard onClick={onNewAgent} />
        </div>

        <p className="text-center text-[11px] text-muted-foreground/55">
          Design-only — agents map to real folders in the actual app. Total session cost{" "}
          <span className="font-medium text-muted-foreground/80">{fmtCost(totals.cost)}</span>.
        </p>
      </div>
    </ScrollArea>
  )
}

function Stat({
  label,
  value,
  icon: Icon,
  color,
  live,
}: {
  label: string
  value: string
  icon: typeof FolderGit2
  color?: string
  live?: boolean
}) {
  return (
    <div className="flex flex-col gap-1.5 rounded-xl border border-border bg-card px-4 py-3 card-shadow">
      <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <Icon className="size-3.5" />
        {label}
      </span>
      <span className="flex items-center gap-1.5">
        <span
          className="text-[22px] font-semibold tabular-nums leading-none"
          style={{ color: color ?? "var(--foreground)" }}
        >
          {value}
        </span>
        {live && (
          <span className="relative flex size-1.5">
            <span className="absolute inline-flex size-full animate-ping rounded-full bg-[var(--interactive)] opacity-70" />
            <span className="relative inline-flex size-1.5 rounded-full bg-[var(--interactive)]" />
          </span>
        )}
      </span>
    </div>
  )
}

function AgentCard({
  agent,
  onOpen,
  onManage,
}: {
  agent: Agent
  onOpen: () => void
  onManage: () => void
}) {
  const s = statusMeta[agent.status]
  const stats = realmStats(agent.id)
  const accent = accentVar[agent.accent]

  return (
    <div className="group flex flex-col gap-3 rounded-xl border border-border bg-card p-4 card-shadow transition-colors hover:border-[color-mix(in_oklab,var(--signal)_45%,transparent)]">
      {/* head */}
      <div className="flex items-center gap-3">
        <span
          className="flex size-10 shrink-0 items-center justify-center rounded-lg"
          style={{ background: `color-mix(in oklab, ${accent} 16%, transparent)`, color: accent }}
        >
          <FolderGit2 className="size-5" />
        </span>
        <div className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className="truncate text-[14px] font-semibold text-foreground/90">{agent.name}</span>
          <span className="truncate font-mono text-[10.5px] text-muted-foreground/65">
            {agent.folder}
          </span>
        </div>
        <span
          className="inline-flex shrink-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[10.5px] font-medium"
          style={{ background: `color-mix(in oklab, ${s.color} 14%, transparent)`, color: s.color }}
        >
          <span
            className={cn("size-1.5 rounded-full", agent.status === "working" && "animate-pulse")}
            style={{ background: s.color }}
          />
          {s.label}
        </span>
      </div>

      {/* meta row */}
      <div className="flex items-center gap-4 text-[11px] text-muted-foreground">
        <span className="inline-flex items-center gap-1">
          <GitBranch className="size-3.5" />
          {agent.branch}
        </span>
        <span className="inline-flex items-center gap-1">
          <Bot className="size-3.5" />
          {agent.model}
        </span>
        <span className="ml-auto inline-flex items-center gap-1 tabular-nums">
          <Clock className="size-3.5" />
          {agent.lastActivity}
        </span>
      </div>

      {/* thread stats */}
      <div className="flex items-center gap-2">
        <Pill value={stats.total} label="threads" color="var(--muted-foreground)" />
        <Pill value={stats.working} label="working" color="var(--interactive)" dim={stats.working === 0} />
        <Pill value={stats.waiting} label="waiting" color="var(--signal)" dim={stats.waiting === 0} />
        <span className="ml-auto text-[12px] font-semibold tabular-nums text-foreground/80">
          {fmtCost(agent.costUsd)}
        </span>
      </div>

      {/* actions */}
      <div className="mt-0.5 flex items-center gap-2">
        <button
          onClick={onOpen}
          className="flex flex-1 items-center justify-center gap-2 rounded-lg bg-[var(--signal)] px-3 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
        >
          <Rocket className="size-4" />
          Open{stats.waiting > 0 ? ` · ${stats.waiting} need you` : ""}
        </button>
        <button
          onClick={onManage}
          className="flex items-center justify-center gap-1.5 rounded-lg border border-border bg-muted/40 px-3 py-2 text-[12.5px] font-medium text-foreground/70 transition-colors hover:border-[var(--interactive)]/50 hover:text-[var(--interactive)]"
        >
          <Settings2 className="size-3.5" />
          Manage
        </button>
      </div>
    </div>
  )
}

function Pill({
  value,
  label,
  color,
  dim,
}: {
  value: number
  label: string
  color: string
  dim?: boolean
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium tabular-nums",
        dim && "opacity-45",
      )}
      style={{ background: `color-mix(in oklab, ${color} 12%, transparent)`, color }}
    >
      {value}
      <span className="font-normal opacity-80">{label}</span>
    </span>
  )
}

function NewAgentCard({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="flex min-h-[164px] flex-col items-center justify-center gap-2.5 rounded-xl border border-dashed border-border bg-transparent p-4 text-muted-foreground transition-colors hover:border-[var(--interactive)]/60 hover:text-[var(--interactive)]"
    >
      <span className="flex size-11 items-center justify-center rounded-xl bg-muted/50">
        <FolderPlus className="size-5" />
      </span>
      <span className="text-[13px] font-medium">New agent</span>
      <span className="max-w-[220px] text-center text-[11px] text-muted-foreground/60">
        Browse the filesystem and initialize an agent in any folder.
      </span>
    </button>
  )
}
