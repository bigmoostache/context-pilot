import { Layers, ArrowRight } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { useQueue } from "@/lib/live"
import { PanelFrame } from "../../PanelFrame"

/**
 * Queue panel — tool calls staged for an atomic flush (one cache break
 * instead of N). Shows the ordered queued actions with their index, tool,
 * intent, and a preview of the target, plus the flush/pause affordances.
 */
export function QueuePanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: queueActions = [] } = useQueue(agentId)
  return (
    <PanelFrame
      icon={Layers}
      name="Queue"
      subtitle={`${queueActions.length} actions staged · active`}
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <div className="mb-3 flex items-center gap-2">
        <span className="rounded-md bg-(--ok)/15 px-2 py-1 text-[11px] font-medium text-(--ok)">
          ● Queue active
        </span>
        <span className="text-[11px] text-muted-foreground/70">
          actions accumulate, then flush atomically
        </span>
      </div>

      <ol className="flex flex-col gap-2">
        {queueActions.map((a) => (
          <li
            key={a.index}
            className="card-shadow flex items-center gap-3 rounded-lg border border-border bg-card p-3"
          >
            <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-muted text-[11px] font-semibold text-foreground/70 tabular-nums">
              {a.index}
            </span>
            <div className="flex min-w-0 flex-1 flex-col leading-tight">
              <div className="flex items-center gap-1.5">
                <span className="font-mono text-[12px] font-medium text-(--signal)">{a.tool}</span>
                <ArrowRight className="size-3 text-muted-foreground/50" />
                <span className="truncate text-[11.5px] text-foreground/80">{a.intent}</span>
              </div>
              <span className="truncate font-mono text-[10.5px] text-muted-foreground/65">
                {a.preview}
              </span>
            </div>
          </li>
        ))}
      </ol>

      <div className="mt-4 flex gap-2">
        <button className="flex-1 rounded-md bg-(--signal) px-3 py-1.5 text-[12px] font-medium text-(--primary-foreground)">
          Execute ({queueActions.length})
        </button>
        <button className="rounded-md border border-border bg-card px-3 py-1.5 text-[12px] text-foreground/80">
          Pause
        </button>
      </div>
    </PanelFrame>
  )
}
