import { Plus, Search, X, Archive, ArchiveRestore, ChevronLeft, Pause, Play } from "lucide-react"
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
  /** pause ↔ resume a single thread (T371) */
  onPause: (id: string) => void
  /** open the New Thread dialog */
  onNewThread: () => void
}

/**
 * Flatten markdown to a one-line plain-text snippet for a list-row preview.
 *
 * A thread row shows a single truncated line, so rendering rich markdown there
 * is wrong (headings/lists/code blocks would break the layout) — every chat
 * client shows a flattened text snippet instead. This strips the syntax that
 * would otherwise leak through as literal characters (`## `, `**bold**`, list
 * bullets, links, fenced code, stray HTML tags) and collapses all whitespace
 * to single spaces. Intentionally lightweight (a preview, not a parser): a
 * stray `_` inside an identifier is left alone rather than risk mangling words.
 */
function flattenMarkdown(md: string): string {
  return md
    .replace(/```[\s\S]*?```/g, " ") // drop fenced code blocks
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1") // image → alt text
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1") // link → label
    .replace(/<[^>]+>/g, " ") // strip HTML tags
    .replace(/^\s{0,3}(#{1,6}|>|[-*+]|\d+\.)\s+/gm, "") // heading/quote/bullet markers
    .replace(/(\*\*|\*|__|~~|`)/g, "") // emphasis / code / strike markers
    .replace(/\s+/g, " ")
    .trim()
}

/** Last-message preview text for a thread row + search matching. */
function previewOf(t: ThreadDetail): string {
  // Auto tool-activity traces are collapsed noise — never surface one as the
  // row preview; show the last real message instead.
  let last: ThreadDetail["log"][number] | undefined
  for (let i = t.log.length - 1; i >= 0; i--) {
    const m = t.log[i]
    if (m && !m.auto) {
      last = m
      break
    }
  }
  if (!last) return ""
  if (last.text) return flattenMarkdown(last.text)
  return last.tool ? `⛭ ${last.tool.name}` : last.questions ? "asked a question" : ""
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
  onPause,
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

  /** Sort threads by most recent activity first. */
  const byRecent = (a: ThreadDetail, b: ThreadDetail) =>
    (b.lastActivityMs ?? 0) - (a.lastActivityMs ?? 0)

  /**
   * Sort the "Agent's turn" group focused-first, then by recency (T36). The
   * thread the agent is actively focused on (`focused_thread_id`, surfaced as
   * `t.focused`) is the one most worth seeing at a glance, so it floats to the
   * top of the section regardless of last-activity time.
   */
  const byFocusThenRecent = (a: ThreadDetail, b: ThreadDetail) => {
    const fa = a.focused ? 1 : 0
    const fb = b.focused ? 1 : 0
    if (fa !== fb) return fb - fa
    return byRecent(a, b)
  }

  const mine = visible.filter((t) => t.status === "MY_TURN").sort(byFocusThenRecent)
  const working = visible.filter((t) => t.status === "THEIR_TURN" || t.status === "ACTIVE").sort(byRecent)
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
                {working.length > 0 && <Group label="User turn" count={working.length} />}
                {working.map((t) => (
                  <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} onArchive={onArchive} onPause={onPause} />
                ))}

                {mine.length > 0 && <Group label="Agent's turn" count={mine.length} />}
                {mine.map((t) => (
                  <ThreadRow key={t.id} t={t} selected={t.id === selectedId} onSelect={onSelect} onArchive={onArchive} onPause={onPause} />
                ))}
              </>
            )}

            {showArchived &&
              // Latest-archived first (T277) — most recently active on top.
              [...visible].sort(byRecent).map((t) => (
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
  onPause,
  archived,
}: {
  t: ThreadDetail
  selected: boolean
  onSelect: (id: string) => void
  onArchive: (id: string) => void
  onPause?: (id: string) => void
  archived?: boolean
}) {
  const preview = previewOf(t)
  const isFocused = !archived && t.focused
  const isPaused = !archived && t.paused
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
        {/* line 1 — dot + name + time + overflow menu */}
        <div className="flex items-center gap-2">
          <span
            className={cn("size-2 shrink-0 rounded-full", pulse && !archived && !isPaused && "animate-pulse")}
            style={{ background: archived ? "var(--muted-foreground)" : isPaused ? "var(--warn)" : dot }}
          />
          <span className="truncate text-[13px] font-medium text-foreground/90">{t.name}</span>
          <span className="relative ml-auto shrink-0">
            <span className="text-[10.5px] tabular-nums text-muted-foreground/50 transition-opacity group-hover:opacity-0">
              {t.lastActivity}
            </span>
            <span className="absolute inset-0 flex items-center justify-end gap-1 opacity-0 transition-opacity group-hover:opacity-100">
              <button
                onClick={(e) => { e.stopPropagation(); onArchive(t.id) }}
                className="flex size-5 items-center justify-center rounded-md text-muted-foreground/60 hover:bg-muted hover:text-foreground"
                title={archived ? "Restore" : "Archive"}
              >
                {archived ? <ArchiveRestore className="size-3" /> : <Archive className="size-3" />}
              </button>
              {!archived && onPause && (
                <button
                  onClick={(e) => { e.stopPropagation(); onPause(t.id) }}
                  className="flex size-5 items-center justify-center rounded-md text-muted-foreground/60 hover:bg-muted hover:text-foreground"
                  title={isPaused ? "Resume" : "Pause"}
                >
                  {isPaused ? <Play className="size-3" /> : <Pause className="size-3" />}
                </button>
              )}
            </span>
          </span>
        </div>
        {/* line 2 — badges + preview */}
        <div className="flex items-center gap-1.5 pl-4">
          {isFocused && (
            <span
              className="shrink-0 rounded-full px-1.5 py-px text-[9.5px] font-semibold uppercase tracking-wide"
              style={{ background: "color-mix(in oklab, var(--ok) 18%, transparent)", color: "var(--ok)" }}
            >
              focused
            </span>
          )}
          {isPaused && (
            <span
              className="shrink-0 rounded-full px-1.5 py-px text-[9.5px] font-semibold uppercase tracking-wide"
              style={{ background: "color-mix(in oklab, var(--warn) 18%, transparent)", color: "var(--warn)" }}
            >
              paused
            </span>
          )}
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

    </div>
  )
}
