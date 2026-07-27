import { useEffect, useMemo, useRef, useState } from "react"
import { Search, MessagesSquare, X } from "lucide-react"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import type { ThreadDetail } from "@/lib/types"
import { previewOf } from "@/lib/support/threadMessages"

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
  onSelect,
}: {
  open: boolean
  onClose: () => void
  threads: ThreadDetail[]
  onSelect: (id: string) => void
}) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[min(560px,80vh)] w-[560px] max-w-[92vw] flex-col p-0">
        <DialogTitle className="sr-only">Search threads</DialogTitle>
        {open && <PaletteBody threads={threads} onClose={onClose} onSelect={onSelect} />}
      </DialogContent>
    </Dialog>
  )
}

/** The live palette contents. Mounted fresh on each open (see parent), so its
 *  `query` / `active` state needs no open-reset effect. The active row is
 *  *derived-clamped* at render (`activeIdx`) instead of corrected in an effect,
 *  keeping the only effect a pure DOM scroll-into-view. */
function PaletteBody({
  threads,
  onClose,
  onSelect,
}: {
  threads: ThreadDetail[]
  onClose: () => void
  onSelect: (id: string) => void
}) {
  const [query, setQuery] = useState("")
  const [active, setActive] = useState(0)
  const listRef = useRef<HTMLDivElement>(null)

  // Recent-first, filtered by name + preview. Empty query lists every thread.
  const results = useMemo(() => {
    const q = query.trim().toLowerCase()
    const sorted = threads.toSorted((a, b) => (b.lastActivityMs ?? 0) - (a.lastActivityMs ?? 0))
    if (q === "") return sorted
    return sorted.filter(
      (t) => t.name.toLowerCase().includes(q) || previewOf(t).toLowerCase().includes(q),
    )
  }, [threads, query])

  // Clamp the active index into range as the result set shrinks — derived, so a
  // filter that drops below `active` never points past the end (no effect needed).
  const activeIdx = Math.min(active, Math.max(0, results.length - 1))

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
        setActive(Math.min(activeIdx + 1, results.length - 1))
        break
      }
      case "ArrowUp": {
        e.preventDefault()
        setActive(Math.max(activeIdx - 1, 0))
        break
      }
      case "Enter": {
        e.preventDefault()
        const hit = results[activeIdx]
        if (hit) pick(hit.id)
        break
      }
      default: {
        break
      }
    }
  }

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
          aria-activedescendant={results.length > 0 ? `thread-opt-${activeIdx}` : undefined}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Search threads…"
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
      <div
        ref={listRef}
        id="thread-search-results"
        role="listbox"
        aria-label="Thread search results"
        className="min-h-0 flex-1 overflow-y-auto p-2"
      >
        {results.length === 0 ? (
          <p className="px-2 py-6 text-center text-[12px] text-muted-foreground/55">
            No threads match your search.
          </p>
        ) : (
          results.map((t, i) => (
            <button
              key={t.id}
              id={`thread-opt-${i}`}
              role="option"
              aria-selected={i === activeIdx}
              onMouseMove={() => setActive(i)}
              onClick={() => pick(t.id)}
              className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors ${
                i === activeIdx ? "bg-muted text-foreground" : "text-foreground/75"
              }`}
            >
              <MessagesSquare className="size-4 shrink-0 text-muted-foreground/60" />
              <span className="min-w-0 flex-1 truncate text-[13px]">{t.name}</span>
              {i === activeIdx ? (
                <span className="shrink-0 text-[11px] text-muted-foreground/50">⏎</span>
              ) : (
                <span className="shrink-0 text-[10.5px] text-muted-foreground/45 tabular-nums">
                  {t.lastActivity}
                </span>
              )}
            </button>
          ))
        )}
      </div>

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

/** A small keycap chip for the palette's footer hint row. */
function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded-sm border border-border bg-muted/60 px-1 font-sans text-[10px] text-muted-foreground/70">
      {children}
    </kbd>
  )
}
