import { Bot, Clock, Hash, MessageSquare, PanelsTopLeft } from "lucide-react"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Right rail of the thread-centered view — metadata about the selected thread
 * and a bridge back to the panel-centered cockpit for the working agent.
 */
export function ThreadDetailRail({
  thread,
  onOpenCockpit,
}: {
  thread: ThreadDetail
  onOpenCockpit: () => void
}) {
  const mine = thread.status === "MY_TURN"
  return (
    <aside className="flex w-[260px] shrink-0 flex-col border-l border-border bg-surface">
      <div className="flex h-11 shrink-0 items-center px-4">
        <span className="text-[12px] font-semibold text-muted-foreground">Details</span>
      </div>

      <div className="flex flex-col gap-4 px-4 py-1">
        <div className="flex flex-col gap-2">
          <div className="text-[13.5px] font-semibold text-foreground/90">{thread.name}</div>
          <span
            className={cn(
              "inline-flex w-fit items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium",
              mine
                ? "bg-[var(--signal)]/15 text-[var(--signal)]"
                : "bg-muted text-muted-foreground",
            )}
          >
            <span
              className={cn("size-1.5 rounded-full", mine && "animate-pulse")}
              style={{ background: mine ? "var(--signal)" : "var(--muted-foreground)" }}
            />
            {mine ? "Your turn" : "Agent working"}
          </span>
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex flex-col gap-2.5">
          <Meta icon={Bot} label="Agent" value={thread.agent} />
          <Meta icon={MessageSquare} label="Messages" value={`${thread.log.length}`} />
          <Meta icon={Clock} label="Created" value={thread.createdAt} />
          <Meta icon={Clock} label="Last activity" value={thread.lastActivity} />
          <Meta icon={Hash} label="ID" value={thread.id} />
        </div>

        <div className="h-px bg-border/60" />

        <button
          onClick={onOpenCockpit}
          className="flex items-center justify-center gap-2 rounded-lg border border-border bg-card px-2.5 py-2 text-[12px] font-medium text-foreground/80 transition-colors hover:border-[var(--interactive)]/50 hover:text-[var(--interactive)] card-shadow"
        >
          <PanelsTopLeft className="size-3.5" />
          Open agent cockpit
        </button>
        <p className="px-0.5 text-[11px] leading-relaxed text-muted-foreground/60">
          Inspect this agent's full context — panels, token budget, and cache.
        </p>
      </div>
    </aside>
  )
}

function Meta({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof Bot
  label: string
  value: string
}) {
  return (
    <div className="flex items-center gap-2.5">
      <Icon className="size-4 text-muted-foreground/50" />
      <span className="text-[12px] text-muted-foreground">{label}</span>
      <span className="ml-auto truncate text-[12px] font-medium text-foreground/85">{value}</span>
    </div>
  )
}
