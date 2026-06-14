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
    <aside className="flex w-[256px] shrink-0 flex-col border-l border-border bg-[oklch(0.165_0.006_75)]">
      <div className="flex h-9 shrink-0 items-center border-b border-border px-3">
        <span className="text-[10px] uppercase tracking-[0.16em] text-muted-foreground">
          thread detail
        </span>
      </div>

      <div className="flex flex-col gap-3 px-3 py-3">
        <div>
          <div className="mb-1 text-[13px] font-semibold text-foreground/90">{thread.name}</div>
          <span
            className={cn(
              "inline-flex items-center gap-1.5 rounded-[3px] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider",
            )}
            style={{
              background: mine ? "var(--signal)" : "oklch(0.24 0.008 75)",
              color: mine ? "oklch(0.16 0.02 75)" : "var(--interactive)",
            }}
          >
            <span
              className={cn("size-1.5 rounded-full", mine && "animate-pulse")}
              style={{ background: mine ? "oklch(0.16 0.02 75)" : "var(--interactive)" }}
            />
            {mine ? "your turn" : "agent working"}
          </span>
        </div>

        <div className="h-px bg-border/60" />

        <Meta icon={Bot} label="agent" value={thread.agent} accent="var(--interactive)" />
        <Meta icon={MessageSquare} label="messages" value={`${thread.log.length}`} />
        <Meta icon={Clock} label="created" value={thread.createdAt} />
        <Meta icon={Clock} label="last activity" value={thread.lastActivity} />
        <Meta icon={Hash} label="id" value={thread.id} />

        <div className="h-px bg-border/60" />

        <button
          onClick={onOpenCockpit}
          className="flex items-center justify-center gap-2 rounded-[4px] border border-border bg-[oklch(0.2_0.008_75)] px-2.5 py-2 text-[11px] font-medium text-foreground/80 transition-colors hover:border-[var(--interactive)]/50 hover:text-[var(--interactive)]"
        >
          <PanelsTopLeft className="size-3.5" />
          Open agent cockpit
        </button>
        <p className="px-0.5 text-[10px] leading-relaxed text-muted-foreground/50">
          Inspect this agent's full context — panels, token budget, and cache economics.
        </p>
      </div>
    </aside>
  )
}

function Meta({
  icon: Icon,
  label,
  value,
  accent,
}: {
  icon: typeof Bot
  label: string
  value: string
  accent?: string
}) {
  return (
    <div className="flex items-center gap-2">
      <Icon className="size-3.5 text-muted-foreground/50" />
      <span className="text-[11px] text-muted-foreground/65">{label}</span>
      <span
        className="ml-auto truncate text-[11px] font-medium"
        style={{ color: accent ?? "var(--foreground)" }}
      >
        {value}
      </span>
    </div>
  )
}
