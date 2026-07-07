import { MessagesSquare } from "lucide-react"
import type { ContextPanel, ThreadStatus } from "@/lib/types"
import { useThreads } from "@/lib/live"
import { PanelFrame } from "../../PanelFrame"
import { cn } from "@/lib/utils"

const STATUS_META: Record<ThreadStatus, { label: string; color: string }> = {
  MY_TURN: { label: "Needs you", color: "var(--signal)" },
  ACTIVE: { label: "Active", color: "var(--ok)" },
  THEIR_TURN: { label: "Working", color: "var(--muted-foreground)" },
}

/**
 * Threads panel — the parallel-discussion roster as the cockpit sees
 * it. Each row shows a status dot (Needs you / Active / Working), the thread
 * name, message count, last activity, and the latest message preview.
 */
export function ThreadsPanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: threadDetails = [] } = useThreads(agentId)
  return (
    <PanelFrame
      icon={MessagesSquare}
      name="Threads"
      subtitle={`${threadDetails.length} threads · parallel work`}
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <ul className="flex flex-col gap-2">
        {threadDetails.map((t) => {
          const meta = STATUS_META[t.status]
          const last = t.log.at(-1)
          return (
            <li key={t.id} className="rounded-lg border border-border bg-card p-3 card-shadow">
              <div className="mb-1 flex items-center gap-2">
                <span
                  className={cn(
                    "size-2 shrink-0 rounded-full",
                    t.status === "ACTIVE" && "animate-pulse",
                  )}
                  style={{ background: meta.color }}
                />
                <span className="truncate text-[12.5px] font-medium text-foreground/90">
                  {t.name}
                </span>
                <span
                  className="ml-auto shrink-0 text-[10px] font-medium"
                  style={{ color: meta.color }}
                >
                  {meta.label}
                </span>
              </div>
              <div className="mb-1.5 flex items-center gap-2 text-[10px] text-muted-foreground/65">
                <span className="font-mono">{t.id}</span>
                <span>· {t.log.length} msgs</span>
                <span>· {t.lastActivity}</span>
              </div>
              {last?.text && (
                <p className="truncate text-[11.5px] text-muted-foreground/80">{last.text}</p>
              )}
            </li>
          )
        })}
      </ul>
    </PanelFrame>
  )
}
