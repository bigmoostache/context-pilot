import { status } from "@/lib/mock"
import { fmtCost } from "@/lib/panelMeta"
import type { StreamPhase } from "@/lib/types"

const phaseMeta: Record<StreamPhase, { label: string; color: string }> = {
  ready: { label: "Ready", color: "var(--ok)" },
  streaming: { label: "Streaming", color: "var(--signal)" },
  tooling: { label: "Working", color: "var(--interactive)" },
  blocked: { label: "Blocked", color: "var(--danger)" },
}

/** Minimal status footer — status · agent on the left, cost on the right. */
export function StatusBar() {
  const p = phaseMeta[status.phase]
  return (
    <footer className="vibrancy flex h-8 shrink-0 items-center gap-3 border-t border-border px-4 text-[12px]">
      <span className="flex items-center gap-1.5">
        <span
          className="size-2 rounded-full"
          style={{ background: p.color }}
        />
        <span className="font-medium text-foreground/80">{p.label}</span>
      </span>

      <span className="h-3.5 w-px bg-border" />

      <span className="text-muted-foreground">{status.agent}</span>

      <span className="ml-auto tabular-nums text-muted-foreground">
        {fmtCost(status.costUsd)}
      </span>
    </footer>
  )
}
