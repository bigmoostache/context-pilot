import { useState } from "react"
import {
  Bot,
  Clock,
  FolderGit2,
  GitBranch,
  MessagesSquare,
  Plus,
  Rocket,
  Sparkles,
} from "lucide-react"
import { agents } from "@/lib/mock"
import { accentVar, fmtCost } from "@/lib/panelMeta"
import type { Agent, AgentStatus, FsNode } from "@/lib/types"
import { cn } from "@/lib/utils"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs you", color: "var(--signal)" },
  idle: { label: "Idle", color: "var(--muted-foreground)" },
}

/**
 * Right pane of the Agents launcher. Two faces driven by the selected fs node:
 *  • a folder that hosts an agent → agent dashboard + "Open workspace"
 *  • a plain folder → an "Initialize agent here" form (name + model)
 *  • a file or nothing → a calm empty state
 */
export function AgentDetail({
  node,
  onOpenAgent,
}: {
  node: FsNode | null
  onOpenAgent: (id: string) => void
}) {
  if (!node) return <EmptyState />
  if (node.kind === "file") return <FileState node={node} />

  const agent = node.agentId ? agents.find((a) => a.id === node.agentId) : undefined
  if (agent) return <AgentDashboard agent={agent} onOpen={() => onOpenAgent(agent.id)} />
  return <CreateAgent node={node} />
}

function Section({ children }: { children: React.ReactNode }) {
  return <main className="flex min-w-0 flex-1 flex-col bg-background">{children}</main>
}

function AgentDashboard({ agent, onOpen }: { agent: Agent; onOpen: () => void }) {
  const s = statusMeta[agent.status]
  return (
    <Section>
      <div className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-5">
        <span className="text-[12.5px] font-semibold text-foreground/85">Agent workspace</span>
      </div>

      <div className="mx-auto flex w-full max-w-[640px] flex-col gap-6 px-6 py-8">
        {/* identity */}
        <div className="flex items-center gap-4">
          <span
            className="flex size-14 items-center justify-center rounded-xl"
            style={{
              background: `color-mix(in oklab, ${accentVar[agent.accent]} 16%, transparent)`,
              color: accentVar[agent.accent],
            }}
          >
            <FolderGit2 className="size-7" />
          </span>
          <div className="flex min-w-0 flex-col gap-1">
            <h2 className="text-[20px] font-semibold tracking-tight text-foreground">{agent.name}</h2>
            <span className="truncate font-mono text-[12px] text-muted-foreground">{agent.folder}</span>
          </div>
          <span
            className="ml-auto inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11.5px] font-medium"
            style={{ background: `color-mix(in oklab, ${s.color} 14%, transparent)`, color: s.color }}
          >
            <span
              className={cn("size-1.5 rounded-full", agent.status === "working" && "animate-pulse")}
              style={{ background: s.color }}
            />
            {s.label}
          </span>
        </div>

        {/* metric grid */}
        <div className="grid grid-cols-2 gap-3">
          <Metric icon={GitBranch} label="Branch" value={agent.branch} />
          <Metric icon={Bot} label="Model" value={agent.model} />
          <Metric icon={MessagesSquare} label="Threads" value={`${agent.threads} open`} />
          <Metric icon={Clock} label="Last activity" value={agent.lastActivity} />
        </div>

        <div className="flex items-center justify-between rounded-xl border border-border bg-card px-4 py-3 card-shadow">
          <div className="flex flex-col">
            <span className="text-[11px] text-muted-foreground">Session cost</span>
            <span className="text-[16px] font-semibold tabular-nums text-foreground">
              {fmtCost(agent.costUsd)}
            </span>
          </div>
          <button
            onClick={onOpen}
            className="flex items-center gap-2 rounded-lg bg-[var(--signal)] px-4 py-2 text-[13px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
          >
            <Rocket className="size-4" />
            Open workspace
          </button>
        </div>
      </div>
    </Section>
  )
}

function Metric({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof GitBranch
  label: string
  value: string
}) {
  return (
    <div className="flex flex-col gap-1 rounded-xl border border-border bg-card px-4 py-3 card-shadow">
      <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <Icon className="size-3.5" />
        {label}
      </span>
      <span className="truncate text-[13.5px] font-medium text-foreground/90">{value}</span>
    </div>
  )
}

function CreateAgent({ node }: { node: FsNode }) {
  const [name, setName] = useState(node.name)
  const models = ["claude-opus-4-8", "claude-sonnet-4-6", "claude-fable-5"]
  const [model, setModel] = useState(models[0])

  return (
    <Section>
      <div className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-5">
        <span className="text-[12.5px] font-semibold text-foreground/85">Initialize agent</span>
      </div>

      <div className="mx-auto flex w-full max-w-[560px] flex-col gap-6 px-6 py-8">
        <div className="flex items-start gap-3">
          <span className="flex size-11 items-center justify-center rounded-xl bg-[var(--interactive)]/12 text-[var(--interactive)]">
            <Sparkles className="size-5" />
          </span>
          <div className="flex flex-col gap-1">
            <h2 className="text-[17px] font-semibold tracking-tight text-foreground">
              No agent here yet
            </h2>
            <p className="text-[12.5px] leading-relaxed text-muted-foreground">
              Create an agent in{" "}
              <span className="font-mono text-foreground/80">{node.path}</span>. One agent owns one
              folder — its threads, context, and history live here.
            </p>
          </div>
        </div>

        <Field label="Agent name">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full rounded-lg border border-border bg-card px-3 py-2 text-[13px] text-foreground outline-none transition-colors focus:border-[var(--interactive)]/60"
          />
        </Field>

        <Field label="Folder">
          <div className="w-full rounded-lg border border-dashed border-border bg-muted/40 px-3 py-2 font-mono text-[12px] text-muted-foreground">
            {node.path}
          </div>
        </Field>

        <Field label="Model">
          <div className="flex flex-wrap gap-2">
            {models.map((m) => (
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

        <button className="flex items-center justify-center gap-2 rounded-lg bg-[var(--interactive)] px-4 py-2.5 text-[13px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105">
          <Plus className="size-4" />
          Create agent in this folder
        </button>
        <p className="-mt-2 text-center text-[11px] text-muted-foreground/60">
          Design-only — wiring an agent to a real folder is the backend's job.
        </p>
      </div>
    </Section>
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

function FileState({ node }: { node: FsNode }) {
  return (
    <Section>
      <div className="flex flex-1 flex-col items-center justify-center gap-2 text-center">
        <span className="font-mono text-[12px] text-muted-foreground/70">{node.path}</span>
        <p className="max-w-[320px] text-[12.5px] text-muted-foreground">
          Files are shown for navigation only. Select a{" "}
          <span className="text-foreground/80">folder</span> to open or create an agent.
        </p>
      </div>
    </Section>
  )
}

function EmptyState() {
  return (
    <Section>
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
        <span className="flex size-12 items-center justify-center rounded-2xl bg-muted text-muted-foreground/60">
          <FolderGit2 className="size-6" />
        </span>
        <p className="max-w-[300px] text-[13px] text-muted-foreground">
          Pick a folder on the left to open its agent — or create one where none exists yet.
        </p>
      </div>
    </Section>
  )
}
