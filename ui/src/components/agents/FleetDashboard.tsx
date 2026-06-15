import { useState } from "react"
import {
  Archive,
  Bot,
  Clock,
  FolderGit2,
  FolderPlus,
  GitBranch,
  MessagesSquare,
  Rocket,
  Settings2,
  Sparkles,
  X,
} from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { agents, threadDetails } from "@/lib/mock"
import { accentVar, fmtCost } from "@/lib/panelMeta"
import type { Agent, AgentStatus } from "@/lib/types"
import { cn } from "@/lib/utils"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs input", color: "var(--signal)" },
  idle: { label: "Standby", color: "var(--muted-foreground)" },
}

const MODELS = ["claude-opus-4-8", "claude-sonnet-4-6", "claude-fable-5"]

/** Thread tally for an agent's realm — single source of truth = threadDetails. */
function realmStats(agentId: string) {
  const threads = threadDetails.filter((t) => t.agentId === agentId)
  return {
    total: threads.length,
    working: threads.filter((t) => t.status === "THEIR_TURN").length,
    waiting: threads.filter((t) => t.status === "MY_TURN").length,
  }
}

type Modal = { mode: "create" } | { mode: "manage"; agent: Agent } | null

/**
 * Fleet welcome dashboard — mission control and the SOLE place agents are
 * managed. Aggregate stats, a card per agent (1 agent = 1 folder), and the
 * create / manage flows (the per-agent views no longer touch agent management).
 */
export function FleetDashboard({ onOpenAgent }: { onOpenAgent: (id: string) => void }) {
  const [modal, setModal] = useState<Modal>(null)
  const [toast, setToast] = useState<string | null>(null)

  const flash = (m: string) => {
    setToast(m)
    window.setTimeout(() => setToast(null), 2200)
  }

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
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ScrollArea className="min-h-0 flex-1 bg-background">
        <div className="mx-auto flex w-full max-w-[940px] flex-col gap-7 px-8 py-9">
          <header className="flex items-end justify-between gap-4">
            <div className="flex flex-col gap-1.5">
              <span className="label">Mission control</span>
              <h1 className="text-[24px] font-semibold tracking-tight text-foreground">
                Your agents
              </h1>
              <p className="text-[13px] text-muted-foreground">
                Each agent lives in one folder — its realm. Manage them here; everything else
                happens inside the agent.
              </p>
            </div>
            <button
              onClick={() => setModal({ mode: "create" })}
              className="flex shrink-0 items-center gap-2 rounded-lg bg-[var(--interactive)] px-3.5 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
            >
              <FolderPlus className="size-4" />
              New agent
            </button>
          </header>

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

          <p className="text-center text-[11px] text-muted-foreground/55">
            Design-only — agents map to real folders in the actual app. Total session cost{" "}
            <span className="font-medium text-muted-foreground/80">{fmtCost(totals.cost)}</span>.
          </p>
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
          <GitBranch className="size-3.5" />
          {agent.branch}
        </span>
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

// ── create / manage modal ─────────────────────────────────────────
function AgentModal({
  modal,
  onClose,
  onFlash,
}: {
  modal: { mode: "create" } | { mode: "manage"; agent: Agent }
  onClose: () => void
  onFlash: (m: string) => void
}) {
  const isManage = modal.mode === "manage"
  const agent = isManage ? modal.agent : undefined
  const [name, setName] = useState(agent?.name ?? "")
  const [folder, setFolder] = useState(agent?.folder ?? "~/code/")
  const [model, setModel] = useState(agent?.model ?? MODELS[0])

  return (
    <div
      className="absolute inset-0 z-40 flex items-center justify-center bg-black/30 backdrop-blur-[2px]"
      onClick={onClose}
    >
      <div
        className="flex w-[440px] flex-col gap-5 rounded-2xl border border-border bg-popover p-6 pop-shadow"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start gap-3">
          <span
            className={cn(
              "flex size-10 items-center justify-center rounded-xl",
              isManage ? "bg-[var(--signal)]/12 text-[var(--signal)]" : "bg-[var(--interactive)]/12 text-[var(--interactive)]",
            )}
          >
            {isManage ? <Settings2 className="size-5" /> : <Sparkles className="size-5" />}
          </span>
          <div className="flex flex-1 flex-col">
            <h3 className="text-[16px] font-semibold tracking-tight text-foreground">
              {isManage ? `Manage ${agent?.name}` : "New agent"}
            </h3>
            <p className="text-[12px] text-muted-foreground">
              {isManage
                ? "Rename, switch model, or archive this agent."
                : "One agent owns one folder — its realm for the whole session."}
            </p>
          </div>
          <button
            onClick={onClose}
            className="flex size-7 items-center justify-center rounded-md text-muted-foreground/60 hover:bg-muted/60 hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>

        <Field label="Agent name">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my-project"
            className="w-full rounded-lg border border-border bg-card px-3 py-2 text-[13px] text-foreground outline-none transition-colors focus:border-[var(--interactive)]/60"
          />
        </Field>

        <Field label="Folder (realm)">
          {isManage ? (
            <div className="w-full rounded-lg border border-dashed border-border bg-muted/40 px-3 py-2 font-mono text-[12px] text-muted-foreground">
              {folder}
            </div>
          ) : (
            <input
              value={folder}
              onChange={(e) => setFolder(e.target.value)}
              className="w-full rounded-lg border border-border bg-card px-3 py-2 font-mono text-[12px] text-foreground outline-none transition-colors focus:border-[var(--interactive)]/60"
            />
          )}
        </Field>

        <Field label="Model">
          <div className="flex flex-wrap gap-2">
            {MODELS.map((m) => (
              <button
                key={m}
                onClick={() => setModel(m)}
                className={cn(
                  "rounded-lg border px-3 py-1.5 text-[12px] font-medium transition-colors",
                  m === model
                    ? "border-[var(--interactive)] bg-[var(--interactive)]/10 text-foreground"
                    : "border-border bg-card text-foreground/70 hover:border-[var(--interactive)]/40",
                )}
              >
                {m}
              </button>
            ))}
          </div>
        </Field>

        <div className="mt-1 flex items-center gap-2">
          {isManage && (
            <button
              onClick={() => {
                onFlash(`Archived ${agent?.name} (design only)`)
                onClose()
              }}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--danger)]/40 px-3 py-2 text-[12.5px] font-medium text-[var(--danger)] transition-colors hover:bg-[var(--danger)]/10"
            >
              <Archive className="size-3.5" />
              Archive
            </button>
          )}
          <button
            onClick={() => {
              onFlash(
                isManage
                  ? `Saved changes to ${name || agent?.name} (design only)`
                  : `Created agent “${name || "untitled"}” in ${folder} (design only)`,
              )
              onClose()
            }}
            className="ml-auto flex items-center gap-2 rounded-lg bg-[var(--interactive)] px-4 py-2 text-[13px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
          >
            {isManage ? "Save changes" : "Create agent"}
          </button>
        </div>
      </div>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      {children}
    </label>
  )
}
