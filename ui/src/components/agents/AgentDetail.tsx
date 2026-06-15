import { useState } from "react"
import {
  Bot,
  ChevronRight,
  Clock,
  File as FileIcon,
  FileCode,
  Folder,
  FolderGit2,
  GitBranch,
  Lock,
  MessagesSquare,
  Plus,
  Rocket,
  Sparkles,
} from "lucide-react"
import { agents, fileTree, threadDetails } from "@/lib/mock"
import { accentVar, fmtCost } from "@/lib/panelMeta"
import type { Agent, AgentStatus, FsNode } from "@/lib/types"
import { cn } from "@/lib/utils"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs you", color: "var(--signal)" },
  idle: { label: "Idle", color: "var(--muted-foreground)" },
}

/** Depth-first search for a node by its path. */
function findByPath(node: FsNode, path: string): FsNode | null {
  if (node.path === path) return node
  for (const child of node.children ?? []) {
    const hit = findByPath(child, path)
    if (hit) return hit
  }
  return null
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
  // Threads that live inside this agent's realm (single source of truth).
  const realmThreads = threadDetails.filter((t) => t.agentId === agent.id)
  const working = realmThreads.filter((t) => t.status === "THEIR_TURN").length
  const needsYou = realmThreads.filter((t) => t.status === "MY_TURN").length
  const realmNode = findByPath(fileTree, agent.folder)
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
          <Metric
            icon={MessagesSquare}
            label="Threads"
            value={`${realmThreads.length} · ${working} working`}
          />
          <Metric icon={Clock} label="Last activity" value={agent.lastActivity} />
        </div>

        {/* realm — the folder the agent is confined to */}
        <div className="flex flex-col gap-2 rounded-xl border border-border bg-card px-4 py-3 card-shadow">
          <div className="flex items-center gap-2">
            <Lock className="size-3.5 text-muted-foreground" />
            <span className="text-[11.5px] font-semibold text-foreground/80">Realm</span>
            <span className="font-mono text-[11px] text-muted-foreground/70">{agent.folder}</span>
            <span className="ml-auto text-[10.5px] text-muted-foreground/60">
              confined — the agent can't go out
            </span>
          </div>
          <div className="rounded-lg bg-muted/40 px-1.5 py-1.5">
            {realmNode ? (
              <RealmTree node={realmNode} depth={0} rootPath={realmNode.path} />
            ) : (
              <span className="px-2 text-[12px] text-muted-foreground/60">
                Folder not indexed.
              </span>
            )}
          </div>
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
            Open workspace{needsYou > 0 ? ` · ${needsYou} need you` : ""}
          </button>
        </div>
      </div>
    </Section>
  )
}

/**
 * Compact, read-only file tree for the agent's realm. Rooted at the agent's
 * folder — there is deliberately no way to navigate above it, expressing the
 * "an agent can't leave its folder" boundary.
 */
function RealmTree({
  node,
  depth,
  rootPath,
}: {
  node: FsNode
  depth: number
  rootPath: string
}) {
  const isRoot = node.path === rootPath
  const [open, setOpen] = useState(true)
  const isDir = node.kind === "dir"
  const FileGlyph =
    node.name.endsWith(".rs") || node.name.endsWith(".ts") || node.name.endsWith(".lean")
      ? FileCode
      : FileIcon

  return (
    <div>
      <button
        type="button"
        onClick={() => isDir && setOpen((o) => !o)}
        className={cn(
          "flex w-full items-center gap-1.5 rounded-md py-0.5 pr-2 text-left transition-colors",
          isDir ? "hover:bg-muted/60" : "cursor-default",
        )}
        style={{ paddingLeft: `${depth * 14 + 6}px` }}
      >
        {isDir ? (
          <ChevronRight
            className={cn(
              "size-3.5 shrink-0 text-muted-foreground/50 transition-transform",
              open && "rotate-90",
            )}
          />
        ) : (
          <span className="w-3.5 shrink-0" />
        )}
        {isDir ? (
          <Folder
            className="size-4 shrink-0"
            style={isRoot ? { color: "var(--signal)" } : { color: "var(--warn)" }}
          />
        ) : (
          <FileGlyph className="size-4 shrink-0 text-muted-foreground/70" />
        )}
        <span
          className={cn(
            "min-w-0 flex-1 truncate text-[12px]",
            isRoot ? "font-semibold text-foreground/90" : "text-foreground/80",
          )}
        >
          {isRoot ? `${node.name}/` : node.name}
        </span>
        {isRoot && (
          <span className="shrink-0 rounded-full bg-[var(--signal)]/15 px-1.5 py-px text-[9.5px] font-semibold text-[var(--signal)]">
            realm root
          </span>
        )}
      </button>
      {isDir && open && node.children && node.children.length > 0 && (
        <div>
          {node.children.map((child) => (
            <RealmTree key={child.path} node={child} depth={depth + 1} rootPath={rootPath} />
          ))}
        </div>
      )}
    </div>
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
