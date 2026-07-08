import { useRef } from "react"
import type { FinderNode } from "@/lib/types"

interface KeyboardDeps {
  sorted: FinderNode[]
  children: FinderNode[]
  focusPath: string | null
  selected: Set<string>
  menuOpen: boolean
  setFocusPath: (p: string | null) => void
  setSelected: (s: Set<string>) => void
  setAnchor: (p: string | null) => void
  setPreview: (n: FinderNode | null) => void
  setPreviewOpen: (fn: (o: boolean) => boolean) => void
  setMenu: (m: null) => void
  startRename: (node: FinderNode) => void
  open: (node: FinderNode) => void
  goUp: () => void
  trashPaths: (paths: string[]) => void
}

// ── Keydown dispatch context + handlers (one small pure fn per concern) ──
//
// The Finder's keyboard surface is a wide command set (arrows, type-ahead,
// Quick Look, rename, open, trash, go-up, select-all, escape). Rather than one
// 39-branch handler, each concern is a `(ctx) => boolean` that reports whether
// it consumed the event; the returned handler builds the context once and runs
// the ordered list, stopping at the first that handles the key. This keeps every
// function within the P8 complexity budget and makes precedence explicit (the
// array order IS the precedence — e.g. ⌘⌫ Trash is tried before plain-Backspace
// go-up so the modifier combo wins).

interface KeyCtx {
  e: React.KeyboardEvent
  d: KeyboardDeps
  /** metaKey || ctrlKey — the "command" modifier. */
  mod: boolean
  /** Index of the focused entry in `sorted`, or -1 when nothing is focused. */
  idx: number
  /** Focus the entry at a clamped index (selects + previews it). */
  focusAt: (i: number) => void
  typeBufRef: React.RefObject<string>
  typeTimerRef: React.RefObject<number | undefined>
}

/** Look up the focused node in the current directory's children. */
function focusedChild(d: KeyboardDeps): FinderNode | undefined {
  return d.focusPath ? d.children.find((c) => c.path === d.focusPath) : undefined
}

/** Arrow keys — move focus one entry (right/down = next, left/up = previous). */
function handleNav({ e, idx, focusAt }: KeyCtx): boolean {
  if (e.key === "ArrowRight" || e.key === "ArrowDown") {
    e.preventDefault()
    focusAt(idx < 0 ? 0 : idx + 1)
    return true
  }
  if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
    e.preventDefault()
    focusAt(idx < 0 ? 0 : idx - 1)
    return true
  }
  return false
}

/**
 * Enter — begin inline rename on the focused entry (macOS Finder convention:
 * Enter renames, double-click opens).
 */
function handleRename({ e, d }: KeyCtx): boolean {
  if (e.key !== "Enter") return false
  e.preventDefault()
  const n = focusedChild(d)
  if (n) d.startRename(n)
  return true
}

/** ⌘/Ctrl+O — open the focused entry (the keyboard open path, since Enter renames). */
function handleOpen({ e, d, mod }: KeyCtx): boolean {
  if (!mod || e.key.toLowerCase() !== "o") return false
  e.preventDefault()
  const n = focusedChild(d)
  if (n) d.open(n)
  return true
}

/** ⌘⌫ — move the current selection (or the focused entry) to Trash. */
function handleTrash({ e, d, mod }: KeyCtx): boolean {
  if (!mod || e.key !== "Backspace") return false
  e.preventDefault()
  const paths = d.selected.size > 0 ? [...d.selected] : d.focusPath ? [d.focusPath] : []
  if (paths.length > 0) d.trashPaths(paths)
  return true
}

/** Backspace / ⌘↑ — navigate to the parent directory. */
function handleGoUp({ e, mod, d }: KeyCtx): boolean {
  if (e.key !== "Backspace" && !(mod && e.key === "ArrowUp")) return false
  e.preventDefault()
  d.goUp()
  return true
}

/** Space — toggle the Quick Look preview. */
function handleQuickLook({ e, d }: KeyCtx): boolean {
  if (e.key !== " ") return false
  e.preventDefault()
  d.setPreviewOpen((o) => !o)
  return true
}

/** ⌘A — select every visible entry. */
function handleSelectAll({ e, d, mod }: KeyCtx): boolean {
  if (!mod || e.key.toLowerCase() !== "a") return false
  e.preventDefault()
  d.setSelected(new Set(d.sorted.map((n) => n.path)))
  return true
}

/** Escape — close an open context menu, else clear the selection. */
function handleEscape({ e, d }: KeyCtx): boolean {
  if (e.key !== "Escape") return false
  if (d.menuOpen) {
    d.setMenu(null)
  } else {
    d.setSelected(new Set())
    d.setFocusPath(null)
  }
  return true
}

/** Printable key — extend the type-ahead buffer and jump to the first match. */
function handleTypeAhead(ctx: KeyCtx): boolean {
  const { e, d, typeBufRef, typeTimerRef } = ctx
  if (e.key.length !== 1 || e.metaKey || e.ctrlKey || e.altKey) return false
  typeBufRef.current += e.key.toLowerCase()
  window.clearTimeout(typeTimerRef.current)
  typeTimerRef.current = window.setTimeout(() => (typeBufRef.current = ""), 700)
  const hit = d.sorted.find((n) => n.name.toLowerCase().startsWith(typeBufRef.current))
  if (hit) {
    d.setFocusPath(hit.path)
    d.setSelected(new Set([hit.path]))
    d.setAnchor(hit.path)
    d.setPreview(hit)
  }
  return true
}

// Ordered precedence: modifier combos before their bare-key equivalents (⌘⌫
// Trash before plain-Backspace go-up; ⌘O/⌘A before type-ahead's printable-key
// catch), so the more specific binding always wins.
const HANDLERS: ((ctx: KeyCtx) => boolean)[] = [
  handleNav,
  handleRename,
  handleOpen,
  handleTrash,
  handleGoUp,
  handleQuickLook,
  handleSelectAll,
  handleEscape,
  handleTypeAhead,
]

/**
 * The Finder surface's keyboard control: arrow navigation, type-ahead, Space
 * Quick Look, Enter→rename (macOS convention) with ⌘/Ctrl+O to open, ⌘⌫ Trash,
 * Backspace go-up, ⌘A select-all, Esc clear. Returns the `onKeyDown` handler
 * bound to the surface; a type-ahead buffer is kept in a private ref.
 */
export function useFinderKeyboard(d: KeyboardDeps) {
  const typeBufRef = useRef("")
  const typeTimerRef = useRef<number | undefined>(undefined)

  return (e: React.KeyboardEvent) => {
    const tag = (e.target as HTMLElement).tagName
    if (tag === "INPUT" || tag === "TEXTAREA") return

    const idx = d.focusPath ? d.sorted.findIndex((n) => n.path === d.focusPath) : -1
    const focusAt = (i: number) => {
      const n = d.sorted[Math.max(0, Math.min(d.sorted.length - 1, i))]
      if (!n) return
      d.setFocusPath(n.path)
      d.setSelected(new Set([n.path]))
      d.setAnchor(n.path)
      d.setPreview(n)
    }

    const ctx: KeyCtx = {
      e,
      d,
      mod: e.metaKey || e.ctrlKey,
      idx,
      focusAt,
      typeBufRef,
      typeTimerRef,
    }
    for (const h of HANDLERS) {
      if (h(ctx)) return
    }
  }
}
