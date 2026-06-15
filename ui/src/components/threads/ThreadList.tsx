import { Plus, Search, PanelLeftClose } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Button } from "@/components/ui/button"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"

interface ThreadListProps {
  threads: ThreadDetail[]
  selectedId: string
  onSelect: (id: string) => void
  /** when true the rail animates closed to zero width */
  collapsed: boolean
  onToggleCollapse: () => void
}

/**
 * Left rail of the thread-centered view — a clean, grouped chat sidebar.
 *
 * The agent identity (name / folder / logo) is intentionally *not* repeated
 * here: it already lives in the TopBar. The rail is collapsible — it animates
 * to zero width, and a floating trigger in {@link ThreadsView} re-opens it.
 *
 * Its width (`SIDEBAR_W` = 240px) is shared with the cockpit and Finder rails
 * so the three views line up consistently.
 */
const SIDEBAR_W = 240

export function ThreadList({
  threads,
  selectedId,
  onSelect,
  collapsed,
  onToggleCollapse,
}: ThreadListProps) {
  const mine = threads.filter((t) => t.status === "MY_TURN")
  const theirs = threads.filter((t) => t.status === "THEIR_TURN")
  // THEIR_TURN = the agent is actively working that thread → parallel work.
  const working = theirs.length

  return (
    <aside
      className={cn(
        "flex shrink-0 flex-col overflow-hidden bg-surface transition-[width] duration-200 ease-in-out",
        collapsed ? "w-0 border-r-0" : "w-[240px] border-r border-border",
      )}
    >
      {/* fixed-width inner shell so content doesn't reflow while collapsing */}
      <div
        className="flex h-full flex-col"
        style={{ width: SIDEBAR_W, minWidth: SIDEBAR_W }}
      >
        {/* top bar — thread count, parallelism, collapse */}
        <div className="flex items-center gap-2 px-3 pb-2.5 pt-3">
          <span className="text-[11px] tabular-nums text-muted-foreground">
            {threads.length} thread{threads.length === 1 ? "" : "s"}
          </span>
          {working > 0 && (
            <span
              className="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[10.5px] font-medium"
              style={{
                background: "color-mix(in oklab, var(--interactive) 14%, transparent)",
                color: "var(--interactive)",
              }}
            >
              <span className="relative flex size-1.5">
                <span className="absolute inline-flex size-full animate-ping rounded-full bg-[var(--interactive)] opacity-70" />
                <span className="relative inline-flex size-1.5 rounded-full bg-[var(--interactive)]" />
              </span>
              {working} working
            </span>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={onToggleCollapse}
            title="Collapse sidebar"
            className="ml-auto text-muted-foreground"
          >
            <PanelLeftClose className="size-4" />
          </Button>
        </div>

        {/* new thread + search */}
        <div className="shrink-0 px-3 pb-2">
          <button className="flex w-full items-center justify-center gap-2 rounded-lg bg-[var(--signal)] px-3 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105">
            <Plus className="size-4" />
            New Thread
          </button>
          <div className="mt-2.5 flex items-center gap-2 rounded-lg border border-border bg-card px-2.5 py-1.5 text-[12px] text-muted-foreground/60">
            <Search className="size-3.5" />
            Search
          </div>
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <div className="px-2 py-1">
            {mine.length > 0 && <Group label="Needs you" count={mine.length} />}
            {mine.map((t) => (
              <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} />
            ))}

            {theirs.length > 0 && <Group label="Working in parallel" count={theirs.length} />}
            {theirs.map((t) => (
              <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} />
            ))}
          </div>
        </ScrollArea>
      </div>
    </aside>
  )
}

function Group({ label, count }: { label: string; count: number }) {
  return (
    <div className="flex items-center gap-2 px-2.5 pb-1 pt-3">
      <span className="text-[11px] font-semibold text-muted-foreground">{label}</span>
      <span className="text-[11px] tabular-nums text-muted-foreground/45">{count}</span>
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
        "flex w-full flex-col gap-1 rounded-lg px-2.5 py-2 text-left transition-colors",
        selected ? "bg-card card-shadow" : "hover:bg-muted/60",
      )}
    >
      <div className="flex items-center gap-2">
        <span
          className={cn("size-2 shrink-0 rounded-full", mine && "animate-pulse")}
          style={{ background: mine ? "var(--signal)" : "var(--muted-foreground)" }}
        />
        <span className="truncate text-[13px] font-medium text-foreground/90">{t.name}</span>
        <span className="ml-auto shrink-0 text-[10.5px] tabular-nums text-muted-foreground/50">
          {t.lastActivity}
        </span>
      </div>
      <div className="flex items-center gap-1.5 pl-4">
        <span className="truncate text-[11.5px] text-muted-foreground/70">{preview}</span>
        {t.unread > 0 && (
          <span
            className="ml-auto shrink-0 rounded-full px-1.5 text-[10px] font-semibold tabular-nums text-[var(--primary-foreground)]"
            style={{ background: "var(--signal)" }}
          >
            {t.unread}
          </span>
        )}
      </div>
    </button>
  )
}
