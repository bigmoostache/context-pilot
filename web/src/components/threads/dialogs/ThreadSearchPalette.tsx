import { useEffect, useMemo, useRef, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { Search, MessagesSquare, X } from "lucide-react"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import type { ThreadDetail } from "@/lib/types"
import { previewOf } from "@/lib/support/threadMessages"
import { searchConversations, type ConvHit } from "@/lib/api"

/**
 * Thread search command-palette (T669). A centred, portaled overlay — opened
 * from the sidebar header's search button — that replaces the old always-on
 * inline search field. Type to filter the realm's threads by name + last-message
 * preview; **↑/↓** move the active row, **⏎** opens it, **Esc** dismisses. Rows
 * are also clickable. Built on the Base UI Dialog primitive (focus-trap,
 * scroll-lock, Esc, backdrop). The inner {@link PaletteBody} mounts only while
 * open, so each open starts with fresh query/selection state — no reset effect.
 */
export function ThreadSearchPalette({
  open,
  onClose,
  threads,
  agentId,
  onSelect,
}: {
  open: boolean
  onClose: () => void
  threads: ThreadDetail[]
  agentId: string
  onSelect: (id: string) => void
}) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[min(560px,80vh)] w-[720px] max-w-[92vw] flex-col p-0">
        <DialogTitle className="sr-only">Search threads</DialogTitle>
        {open && (
          <PaletteBody threads={threads} agentId={agentId} onClose={onClose} onSelect={onSelect} />
        )}
      </DialogContent>
    </Dialog>
  )
}

/** A normalized palette row — one shape for both the instant local thread
 *  filter and the server-side hybrid message hits, so the keyboard-nav and
 *  render code stay a single path regardless of which source is showing. */
interface Row {
  /** Unique per-row React key (a thread can appear in several message hits). */
  key: string
  /** Navigation target. */
  threadId: string
  /** Bold first line — the thread name. */
  primary: string
  /** Dimmed second line — last-message preview (local) or the matched
   *  message text (server). */
  secondary: string
  /** Right-aligned meta — relative age (local) or author (server). */
  meta: string
}

/** Minimum query length before the server hybrid search fires. Below it, the
 *  palette shows the instant client-side thread filter only. */
const SERVER_MIN = 2

/** Debounce before a keystroke reaches the server search (ms). */
const DEBOUNCE_MS = 220

/** The live palette contents. Mounted fresh on each open (see parent), so its
 *  `query` / `active` state needs no open-reset effect. Below {@link SERVER_MIN}
 *  chars it filters the loaded threads instantly; at or above it, it fires a
 *  debounced hybrid (keyword+semantic) search over the agent's conversation
 *  index and lists message-level hits. The active row is *derived-clamped* at
 *  render (`activeIdx`); the only effect is a pure DOM scroll-into-view. */
function PaletteBody({
  threads,
  agentId,
  onClose,
  onSelect,
}: {
  threads: ThreadDetail[]
  agentId: string
  onClose: () => void
  onSelect: (id: string) => void
}) {
  const [query, setQuery] = useState("")
  const [active, setActive] = useState(0)
  const [debounced, setDebounced] = useState("")
  const listRef = useRef<HTMLDivElement>(null)

  // Trail the typed query by DEBOUNCE_MS so the server search fires once the
  // user pauses, not on every keystroke.
  useEffect(() => {
    const id = setTimeout(() => setDebounced(query), DEBOUNCE_MS)
    return () => clearTimeout(id)
  }, [query])

  const serverMode = debounced.trim().length >= SERVER_MIN

  // Hybrid search — only when the debounced query is long enough. Keep the
  // previous data on refetch so the list doesn't flash empty between keystrokes.
  const { data: hits, isFetching } = useQuery({
    queryKey: ["convSearch", agentId, debounced],
    queryFn: () => searchConversations(agentId, debounced, { limit: 30 }),
    enabled: serverMode,
    staleTime: 15_000,
    placeholderData: (prev: ConvHit[] | undefined) => prev,
  })

  // Normalize whichever source is active into a flat Row[].
  const rows: Row[] = useMemo(() => {
    if (serverMode) {
      return (hits ?? []).map((h, i) => ({
        key: `${h.thread_id}-${i}`,
        threadId: h.thread_id,
        primary: h.thread_name,
        secondary: h.text,
        meta: h.author,
      }))
    }
    const q = query.trim().toLowerCase()
    const sorted = threads.toSorted((a, b) => (b.lastActivityMs ?? 0) - (a.lastActivityMs ?? 0))
    const filtered =
      q === ""
        ? sorted
        : sorted.filter(
            (t) => t.name.toLowerCase().includes(q) || previewOf(t).toLowerCase().includes(q),
          )
    return filtered.map((t) => ({
      key: t.id,
      threadId: t.id,
      primary: t.name,
      secondary: previewOf(t),
      meta: t.lastActivity ?? "",
    }))
  }, [serverMode, hits, threads, query])

  // Clamp the active index into range as the result set shrinks — derived, so a
  // filter that drops below `active` never points past the end (no effect needed).
  const activeIdx = Math.min(active, Math.max(0, rows.length - 1))

  // Keep the active row scrolled into view (pure DOM side effect, no setState).
  useEffect(() => {
    listRef.current?.querySelector(`#thread-opt-${activeIdx}`)?.scrollIntoView({ block: "nearest" })
  }, [activeIdx])

  const pick = (id: string) => {
    onSelect(id)
    onClose()
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    switch (e.key) {
      case "ArrowDown": {
        e.preventDefault()
        setActive(Math.min(activeIdx + 1, rows.length - 1))
        break
      }
      case "ArrowUp": {
        e.preventDefault()
        setActive(Math.max(activeIdx - 1, 0))
        break
      }
      case "Enter": {
        e.preventDefault()
        const hit = rows[activeIdx]
        if (hit) pick(hit.threadId)
        break
      }
      default: {
        break
      }
    }
  }

  // Distinguish "still searching" from "searched, nothing found" so the empty
  // state never lies while a request is in flight.
  const searching = serverMode && isFetching && rows.length === 0
  const emptyLabel = searching
    ? "Searching messages…"
    : serverMode
      ? "No messages match your search."
      : "No threads match your search."

  return (
    <>
      {/* search input row */}
      <div className="flex shrink-0 items-center gap-2.5 px-4 py-3">
        <Search className="size-4 shrink-0 text-muted-foreground/70" />
        <input
          autoFocus
          role="combobox"
          aria-expanded
          aria-controls="thread-search-results"
          aria-activedescendant={rows.length > 0 ? `thread-opt-${activeIdx}` : undefined}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value)
            setActive(0)
          }}
          onKeyDown={onKeyDown}
          placeholder="Search threads and messages…"
          className="min-w-0 flex-1 bg-transparent text-[14px] text-foreground/90 outline-none placeholder:text-muted-foreground/55"
        />
        <button
          onClick={onClose}
          title="Close"
          className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
        >
          <X className="size-4" />
        </button>
      </div>

      <div className="h-px shrink-0 bg-border" />

      {/* results */}
      <ResultList
        listRef={listRef}
        rows={rows}
        activeIdx={activeIdx}
        emptyLabel={emptyLabel}
        onHover={setActive}
        onPick={pick}
      />

      {/* footer hints */}
      <div className="flex shrink-0 items-center gap-4 border-t border-border px-4 py-2 text-[11px] text-muted-foreground/55">
        <span className="flex items-center gap-1.5">
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd>
          navigate
        </span>
        <span className="flex items-center gap-1.5">
          <Kbd>⏎</Kbd>
          open
        </span>
        <span className="flex items-center gap-1.5">
          <Kbd>esc</Kbd>
          close
        </span>
      </div>
    </>
  )
}

/** The scrollable result rows (extracted from {@link PaletteBody} to keep it
 *  under the per-function line cap). Renders the empty/searching placeholder or
 *  the normalized {@link Row} list; the active row is highlighted and marked
 *  `aria-selected`. Hover sets the active index; click picks the thread. */
function ResultList({
  listRef,
  rows,
  activeIdx,
  emptyLabel,
  onHover,
  onPick,
}: {
  listRef: React.RefObject<HTMLDivElement | null>
  rows: Row[]
  activeIdx: number
  emptyLabel: string
  onHover: (i: number) => void
  onPick: (id: string) => void
}) {
  return (
    <div
      ref={listRef}
      id="thread-search-results"
      role="listbox"
      aria-label="Thread search results"
      className="min-h-0 flex-1 overflow-y-auto p-2"
    >
      {rows.length === 0 ? (
        <p className="px-2 py-6 text-center text-[12px] text-muted-foreground/55">{emptyLabel}</p>
      ) : (
        rows.map((row, i) => (
          <button
            key={row.key}
            id={`thread-opt-${i}`}
            role="option"
            aria-selected={i === activeIdx}
            onMouseMove={() => onHover(i)}
            onClick={() => onPick(row.threadId)}
            className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors ${
              i === activeIdx ? "bg-muted text-foreground" : "text-foreground/75"
            }`}
          >
            <MessagesSquare className="size-4 shrink-0 text-muted-foreground/60" />
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-[13px]">{row.primary}</span>
              {row.secondary !== "" && (
                <span className="truncate text-[11px] text-muted-foreground/55">
                  {row.secondary}
                </span>
              )}
            </span>
            {i === activeIdx ? (
              <span className="shrink-0 text-[11px] text-muted-foreground/50">⏎</span>
            ) : (
              <span className="shrink-0 text-[10.5px] text-muted-foreground/45 tabular-nums">
                {row.meta}
              </span>
            )}
          </button>
        ))
      )}
    </div>
  )
}

/** A small keycap chip for the palette's footer hint row. */
function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded-sm border border-border bg-muted/60 px-1 font-sans text-[10px] text-muted-foreground/70">
      {children}
    </kbd>
  )
}
