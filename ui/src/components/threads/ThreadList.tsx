import { Plus, Search } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"

interface ThreadListProps {
  threads: ThreadDetail[]
  selectedId: string
  onSelect: (id: string) => void
}

/** Left rail of the thread-centered view — grouped, classic chat sidebar. */
export function ThreadList({ threads, selectedId, onSelect }: ThreadListProps) {
  const mine = threads.filter((t) => t.status === "MY_TURN")
  const theirs = threads.filter((t) => t.status === "THEIR_TURN")

  return (
    <aside className="flex w-[270px] shrink-0 flex-col border-r border-border bg-[oklch(0.165_0.006_75)]">
      {/* header + new thread */}
      <div className="shrink-0 border-b border-border px-3 py-2.5">
        <div className="mb-2 flex items-center justify-between">
          <span className="glow-signal text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--signal)]">
            Threads
          </span>
          <span className="rounded-[2px] bg-card px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground">
            {threads.length}
          </span>
        </div>
        <button className="flex w-full items-center gap-2 rounded-[4px] border border-[var(--signal)]/40 bg-[var(--signal)]/10 px-2.5 py-1.5 text-[12px] font-medium text-[var(--signal)] transition-colors hover:bg-[var(--signal)]/18">
          <Plus className="size-3.5" />
          New Thread
        </button>
        <div className="mt-2 flex items-center gap-2 rounded-[4px] border border-border bg-[oklch(0.185_0.007_75)] px-2.5 py-1 text-[11px] text-muted-foreground/50">
          <Search className="size-3" />
          search…
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="px-2 py-2">
          <Group label="Needs you" count={mine.length} accent="var(--signal)" />
          {mine.map((t) => (
            <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} />
          ))}

          <div className="h-2" />
          <Group label="In progress" count={theirs.length} accent="var(--interactive)" />
          {theirs.map((t) => (
            <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} />
          ))}
        </div>
      </ScrollArea>
    </aside>
  )
}

function Group({ label, count, accent }: { label: string; count: number; accent: string }) {
  return (
    <div className="flex items-center gap-2 px-2 pb-1 pt-1">
      <span className="text-[10px] uppercase tracking-[0.14em]" style={{ color: accent }}>
        {label}
      </span>
      <span className="text-[10px] tabular-nums text-muted-foreground/45">{count}</span>
      <span className="ml-1 h-px flex-1 bg-border/60" />
    </div>
  )
}

function ThreadRow({
  t,
  selected,
  onSelect,
}: {
  t: ThreadDetail
  selected: boolean
  onSelect: (id: string) => void
}) {
  const mine = t.status === "MY_TURN"
  const last = t.log[t.log.length - 1]
  const preview = last?.text ?? (last?.tool ? `⛭ ${last.tool.name}` : last?.questions ? "asked a question" : "")

  return (
    <button
      onClick={() => onSelect(t.id)}
      className={cn(
        "group relative flex w-full flex-col gap-0.5 rounded-[4px] px-2.5 py-2 text-left transition-colors",
        selected ? "bg-[oklch(0.22_0.009_75)]" : "hover:bg-[oklch(0.19_0.007_75)]",
      )}
    >
      {selected && (
        <span
          className="absolute inset-y-1 left-0 w-[2px] rounded-full"
          style={{ background: mine ? "var(--signal)" : "var(--interactive)" }}
        />
      )}
      <div className="flex items-center gap-2">
        <span
          className={cn("size-1.5 shrink-0 rounded-full", mine && "animate-pulse")}
          style={{
            background: mine ? "var(--signal)" : "var(--interactive)",
            boxShadow: mine ? "0 0 5px var(--signal)" : "none",
          }}
        />
        <span className="truncate text-[12.5px] font-medium text-foreground/90">{t.name}</span>
        <span className="ml-auto shrink-0 text-[9px] tabular-nums text-muted-foreground/45">
          {t.lastActivity}
        </span>
      </div>
      <div className="flex items-center gap-1.5 pl-3.5">
        <span className="truncate text-[11px] text-muted-foreground/65">{preview}</span>
        {t.unread > 0 && (
          <span
            className="ml-auto shrink-0 rounded-full px-1.5 text-[9px] font-bold tabular-nums text-[oklch(0.16_0.02_75)]"
            style={{ background: "var(--signal)" }}
          >
            {t.unread}
          </span>
        )}
      </div>
    </button>
  )
}
