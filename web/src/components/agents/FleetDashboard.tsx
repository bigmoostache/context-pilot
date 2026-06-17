import { useEffect, useState } from "react"
import {
  Bot,
  FolderGit2,
  FolderPlus,
  Rocket,
  Settings2,
} from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { agents } from "@/lib/mock"
import { accentVar, fmtCost } from "@/lib/panelMeta"
import type { Agent, AgentStatus } from "@/lib/types"
import { cn } from "@/lib/utils"
import { FLEET_MAX_W } from "./FleetShell"
import { AgentModal } from "./AgentModal"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs input", color: "var(--signal)" },
  idle: { label: "Standby", color: "var(--muted-foreground)" },
}

type Modal = { mode: "create" } | { mode: "manage"; agent: Agent } | null

/**
 * Fleet welcome dashboard — mission control and the SOLE place agents are
 * managed. Aggregate stats, a card per agent (1 agent = 1 folder), and the
 * create / manage flows (the per-agent views no longer touch agent management).
 */
export function FleetDashboard({
  onOpenAgent,
  autoCreate,
  onAutoCreateConsumed,
}: {
  onOpenAgent: (id: string) => void
  /** When flipped true (e.g. via the TopBar "New agent" entry), open the
   *  create dialog immediately and signal back so the flag can be cleared. */
  autoCreate?: boolean
  onAutoCreateConsumed?: () => void
}) {
  const [modal, setModal] = useState<Modal>(null)
  const [toast, setToast] = useState<string | null>(null)

  // Honour an external "create a new agent" request (from the workspace
  // switcher). Open the dialog in create mode, then consume the flag.
  useEffect(() => {
    if (autoCreate) {
      setModal({ mode: "create" })
      onAutoCreateConsumed?.()
    }
  }, [autoCreate, onAutoCreateConsumed])

  const flash = (m: string) => {
    setToast(m)
    window.setTimeout(() => setToast(null), 2200)
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ScrollArea className="min-h-0 flex-1 bg-background">
        <div className={cn("mx-auto flex w-full flex-col gap-7 px-8 py-9", FLEET_MAX_W)}>
          <header className="flex items-end justify-between gap-4">
            <div className="flex flex-col gap-1.5">
              <h1 className="text-[24px] font-semibold tracking-tight text-foreground">
                Agents
              </h1>
            </div>
            <button
              onClick={() => setModal({ mode: "create" })}
              className="flex shrink-0 items-center gap-2 rounded-lg bg-[var(--interactive)] px-3.5 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
            >
              <FolderPlus className="size-4" />
              New agent
            </button>
          </header>

          <div className="grid grid-cols-1 gap-3.5 md:grid-cols-2">
            {agents.map((a) => (
              <AgentCard
                key={a.id}
                agent={a}
                onOpen={() => onOpenAgent(a.id)}
                onManage={() => setModal({ mode: "manage", agent: a })}
              />
            ))}
            <NewAgentCard onClick={() => setModal({ mode: "create" })} />
          </div>
        </div>
      </ScrollArea>

      {modal && (
        <AgentModal modal={modal} onClose={() => setModal(null)} onFlash={flash} />
      )}

      {toast && (
        <div className="absolute bottom-6 left-1/2 z-30 -translate-x-1/2 rounded-lg border border-border bg-card px-4 py-2 text-[12px] text-foreground/90 pop-shadow">
          {toast}
        </div>
      )}
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
  const accent = accentVar[agent.accent]

  return (
    <div className="group flex flex-col gap-3 rounded-xl border border-border bg-card p-4 card-shadow transition-colors hover:border-[color-mix(in_oklab,var(--signal)_45%,transparent)]">
      <div className="flex items-center gap-3">
        <span
          className="flex size-10 shrink-0 items-center justify-center rounded-lg"
          style={{ background: `color-mix(in oklab, ${accent} 16%, transparent)`, color: accent }}
        >
          <FolderGit2 className="size-5" />
        </span>
        <div className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className="truncate text-[14px] font-semibold text-foreground/90">{agent.name}</span>
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

      {/* one-line summary of what the agent is doing */}
      <p className="line-clamp-2 min-h-[2.4em] text-[12px] leading-snug text-foreground/70">
        {agent.task}
      </p>

      <div className="flex items-center gap-4 text-[11px] text-muted-foreground">
        <span className="inline-flex items-center gap-1">
          <Bot className="size-3.5" />
          {agent.model}
        </span>
        <span className="ml-auto font-semibold tabular-nums text-foreground/80">
          {fmtCost(agent.costUsd)}
        </span>
      </div>

      <div className="mt-0.5 flex items-center gap-2">
        <button
          onClick={onOpen}
          className="flex flex-1 items-center justify-center gap-2 rounded-lg bg-[var(--signal)] px-3 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
        >
          <Rocket className="size-4" />
          Open
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
        Initialize an agent in a folder — its realm for the whole session.
      </span>
    </button>
  )
}

