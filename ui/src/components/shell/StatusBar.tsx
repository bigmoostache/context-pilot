import { Bot, GitBranch, Layers, Sparkles, Zap } from "lucide-react"
import { status } from "@/lib/mock"
import { fmtCost } from "@/lib/panelMeta"
import { cn } from "@/lib/utils"
import type { StreamPhase } from "@/lib/types"

const phaseMeta: Record<StreamPhase, { label: string; color: string }> = {
  ready: { label: "READY", color: "var(--ok)" },
  streaming: { label: "STREAMING", color: "var(--signal)" },
  tooling: { label: "TOOLING", color: "var(--interactive)" },
  blocked: { label: "BLOCKED", color: "var(--danger)" },
}

export function StatusBar() {
  const p = phaseMeta[status.phase]
  return (
    <footer className="flex h-7 shrink-0 items-center gap-2 border-t border-border bg-[oklch(0.185_0.007_75)] px-2 text-[10px]">
      {/* phase badge */}
      <span
        className="flex items-center gap-1.5 rounded-[2px] px-2 py-0.5 font-semibold tracking-[0.12em]"
        style={{ background: `${p.color}1f`, color: p.color }}
      >
        <span
          className="size-1.5 animate-pulse rounded-full"
          style={{ background: p.color, boxShadow: `0 0 5px ${p.color}` }}
        />
        {p.label}
      </span>

      <Chip icon={<Bot className="size-3" />} label={status.agent} color="var(--signal)" />
      {status.skills.map((s) => (
        <Chip key={s} icon={<Sparkles className="size-3" />} label={s} color="var(--interactive)" />
      ))}

      <span className="h-3 w-px bg-border" />

      <Chip icon={<GitBranch className="size-3" />} label={status.branch} />
      <Chip icon={<Layers className="size-3" />} label={`queue ${status.queue}`} />
      <Chip
        icon={<Zap className="size-3" />}
        label={`think ${status.think}`}
        color={status.think < 0 ? "var(--danger)" : "var(--muted-foreground)"}
      />

      <div className="ml-auto flex items-center gap-2">
        <span className="text-muted-foreground/50">
          reverie {status.reverie ? "on" : "off"} · auto {status.autoContinue ? "on" : "off"}
        </span>
        <span className="font-semibold text-[var(--warn)]">{fmtCost(status.costUsd)}</span>
      </div>
    </footer>
  )
}

function Chip({
  icon,
  label,
  color = "var(--muted-foreground)",
}: {
  icon: React.ReactNode
  label: string
  color?: string
}) {
  return (
    <span className={cn("flex items-center gap-1 tabular-nums")} style={{ color }}>
      {icon}
      <span className="max-w-[140px] truncate">{label}</span>
    </span>
  )
}
