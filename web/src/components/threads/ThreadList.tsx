import { Plus, Search, X, Archive, ArchiveRestore, ChevronLeft } from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import type { ThreadDetail } from "@/lib/types"
import { cn } from "@/lib/utils"

interface ThreadListProps {
  /** all of the realm's threads (archived included) — filtering happens here */
  threads: ThreadDetail[]
  selectedId: string
  onSelect: (id: string) => void
  /** live search query (controlled by the parent so it survives collapse) */
  query: string
  onQueryChange: (q: string) => void
  /** archived view toggle */
  showArchived: boolean
  onToggleArchived: (v: boolean) => void
  /** archive ↔ restore a single thread */
  onArchive: (id: string) => void
  /** open the New Thread dialog */
  onNewThread: () => void
}

/** Last-message preview text for a thread row + search matching. */
function previewOf(t: ThreadDetail): string {
  const last = t.log[t.log.length - 1]
  if (!last) return ""
  return last.text ?? (last.tool ? `⛭ ${last.tool.name}` : last.questions ? "asked a question" : "")
}

/**
 * Left rail of the thread-centered view — a clean, grouped chat sidebar.
 *
 * The agent identity (name / folder / logo) is intentionally *not* repeated
 * here: it already lives in the TopBar. The rail is **always open** — there is
 * no collapse affordance (removed per T23).
 *
 * The search box genuinely filters (by name + last-message preview). Threads
 * are grouped by turn-status — **Needs you** (MY_TURN), **Active** (the single
 * green thread the agent is streaming right now), **Working in parallel**
 * (THEIR_TURN) — with an on-demand **Archived** view. Its width is the shared
 * `--sidebar-w` CSS variable so it lines up with every other rail.
 */
export function ThreadList({
  threads,
  selectedId,
  onSelect,
  query,
  onQueryChange,
  showArchived,
  onToggleArchived,
  onArchive,
  onNewThread,
}: ThreadListProps) {
  const q = query.trim().toLowerCase()
  const matches = (t: ThreadDetail) =>
    q === "" || t.name.toLowerCase().includes(q) || previewOf(t).toLowerCase().includes(q)

  const live = threads.filter((t) => !t.archived)
  const archived = threads.filter((t) => t.archived)
  const archivedCount = archived.length

  // search applies to whichever set is on screen
  const visible = (showArchived ? archived : live).filter(matches)

  const mine = visible.filter((t) => t.status === "MY_TURN")
  const working = visible.filter((t) => t.status === "THEIR_TURN" || t.status === "ACTIVE")
  // agent-owned, actively-or-parallel working count (for the header pill)
  const workingCount = live.filter((t) => t.status !== "MY_TURN").length

  return (
    <aside className="flex w-[var(--sidebar-w)] shrink-0 flex-col overflow-hidden border-r border-border bg-surface">
      {/* fixed-width inner shell pinned to the rail width */}
      <div
        className="flex h-full flex-col"
        style={{ width: "var(--sidebar-w)", minWidth: "var(--sidebar-w)" }}
      >
        {/* top bar — context-sensitive: live count + parallelism, or Archived header */}
        <div className="flex items-center gap-2 px-3 pb-2.5 pt-3">
          {showArchived ? (
            <button
              onClick={() => onToggleArchived(false)}
              className="flex items-center gap-1.5 text-[12px] font-medium text-foreground/80 transition-colors hover:text-foreground"
            >
              <ChevronLeft className="size-3.5" />
              Archived
              <span className="tabular-nums text-muted-foreground/50">{archivedCount}</span>
            </button>
          ) : (
            <>
              <span className="text-[11px] tabular-nums text-muted-foreground">
                {live.length} thread{live.length === 1 ? "" : "s"}
              </span>
              {workingCount > 0 && (
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
                  {workingCount} working
                </span>
              )}
            </>
          )}
        </div>

        {/* new thread + search (hidden in archived view — archived is read-only) */}
        {!showArchived && (
          <div className="shrink-0 px-3 pb-2">
            <button
              onClick={onNewThread}
              className="flex w-full items-center justify-center gap-2 rounded-lg bg-[var(--signal)] px-3 py-2 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105"
            >
              <Plus className="size-4" />
              New Thread
            </button>
          </div>
        )}

        {/* search — works in both live and archived views */}
        <div className="shrink-0 px-3 pb-2">
          <div className="flex items-center gap-2 rounded-lg border border-border bg-card px-2.5 py-1.5 text-[12px] focus-within:border-[var(--signal)]/60">
            <Search className="size-3.5 shrink-0 text-muted-foreground/60" />
            <input
              value={query}
              onChange={(e) => onQueryChange(e.target.value)}
              placeholder={showArchived ? "Search archived…" : "Search threads…"}
              className="min-w-0 flex-1 bg-transparent text-foreground/90 placeholder:text-muted-foreground/55 outline-none"
            />
            {query && (
              <button
                onClick={() => onQueryChange("")}
                className="shrink-0 text-muted-foreground/55 transition-colors hover:text-foreground"
                title="Clear"
              >
                <X className="size-3.5" />
              </button>
            )}
          </div>
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <div className="px-2 py-1">
            {visible.length === 0 && (
              <p className="px-2.5 py-6 text-center text-[11.5px] text-muted-foreground/55">
                {q ? "No threads match your search." : showArchived ? "No archived threads." : "No threads yet."}
              </p>
            )}

            {!showArchived && (
              <>
                {mine.length > 0 && <Group label="Needs you" count={mine.length} />}
                {mine.map((t) => (
                  <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} onArchive={onArchive} />
                ))}

                {working.length > 0 && <Group label="Working in parallel" count={working.length} />}
                {working.map((t) => (
                  <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} onArchive={onArchive} />
                ))}
              </>
            )}

            {showArchived &&
              visible.map((t) => (
                <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} onArchive={onArchive} archived />
              ))}
          </div>
        </ScrollArea>

        {/* archived entry point (hidden while already viewing archived) */}
        {!showArchived && archivedCount > 0 && (
          <button
            onClick={() => onToggleArchived(true)}
            className="flex shrink-0 items-center gap-2 border-t border-border px-3.5 py-2.5 text-[12px] text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground"
          >
            <Archive className="size-3.5" />
            Archived
            <span className="ml-auto tabular-nums text-muted-foreground/50">{archivedCount}</span>
          </button>
        )}
      </div>
    </aside>
  )
}

function Group({ label, count, accent }: { label: string; count: number; accent?: string }) {
  return (
    <div className="flex items-center gap-2 px-2.5 pb-1 pt-3">
      <span
        className="text-[11px] font-semibold"
        style={{ color: accent ?? "var(--muted-foreground)" }}
      >
        {label}
      </span>
      <span className="text-[11px] tabular-nums text-muted-foreground/45">{count}</span>
    </div>
  )
}

function ThreadRow({
  t,
  selected,
  onSelect,
  onArchive,
  archived,
}: {
  t: ThreadDetail
  selected: boolean
  onSelect: (id: string) => void
  onArchive: (id: string) => void
  archived?: boolean
}) {
  const preview = previewOf(t)
  const isFocused = !archived && t.focused
  const dot =
    isFocused
      ? "var(--ok)"
      : t.status === "MY_TURN"
        ? "var(--signal)"
        : t.status === "ACTIVE"
          ? "var(--ok)"
          : "var(--muted-foreground)"
  const pulse = isFocused || t.status === "MY_TURN" || t.status === "ACTIVE"

  return (
    <div
      className={cn(
        "group relative flex w-full flex-col gap-1 rounded-lg px-2.5 py-2 text-left transition-colors",
        selected ? "bg-card card-shadow" : "hover:bg-muted/60",
      )}
    >
      <button onClick={() => onSelect(t.id)} className="flex flex-col gap-1 text-left">
        <div className="flex items-center gap-2">
          <span
            className={cn("size-2 shrink-0 rounded-full", pulse && !archived && "animate-pulse")}
            style={{ background: archived ? "var(--muted-foreground)" : dot }}
          />
          <span className="truncate text-[13px] font-medium text-foreground/90">{t.name}</span>
          {isFocused && (
            <span
              className="shrink-0 rounded-full px-1.5 py-px text-[9.5px] font-semibold uppercase tracking-wide"
              style={{ background: "color-mix(in oklab, var(--ok) 18%, transparent)", color: "var(--ok)" }}
            >
              focused
            </span>
          )}
          <span className="ml-auto shrink-0 pr-5 text-[10.5px] tabular-nums text-muted-foreground/50">
            {t.lastActivity}
          </span>
        </div>
        <div className="flex items-center gap-1.5 pl-4">
          <span className="truncate text-[11.5px] text-muted-foreground/70">{preview}</span>
          {!archived && t.unread > 0 && (
            <span
              className="ml-auto shrink-0 rounded-full px-1.5 text-[10px] font-semibold tabular-nums text-[var(--primary-foreground)]"
              style={{ background: "var(--signal)" }}
            >
              {t.unread}
            </span>
          )}
        </div>
      </button>

      {/* hover action — archive (or restore in the archived view) */}
      <button
        onClick={(e) => {
          e.stopPropagation()
          onArchive(t.id)
        }}
        title={archived ? "Restore thread" : "Archive thread"}
        className="absolute right-2 top-2 flex size-6 items-center justify-center rounded-md text-muted-foreground/60 opacity-0 transition-all hover:bg-muted hover:text-foreground group-hover:opacity-100"
      >
        {archived ? <ArchiveRestore className="size-3.5" /> : <Archive className="size-3.5" />}
      </button>
    </div>
  )
}
