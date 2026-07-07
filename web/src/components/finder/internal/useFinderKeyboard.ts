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

/**
 * The Finder surface's keyboard control: arrow navigation, type-ahead, Space
 * Quick Look, Enter→rename (macOS convention) with ⌘/Ctrl+O to open, ⌘⌫ Trash,
 * Backspace go-up, ⌘A select-all, Esc clear. Returns the `onKeyDown` handler
 * bound to the surface; a type-ahead buffer is kept in a private ref.
 */
export function useFinderKeyboard(d: KeyboardDeps) {
  const typeBuf = useRef("")
  const typeTimer = useRef<number | undefined>(undefined)

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

    switch (e.key) {
      case "ArrowRight":
      case "ArrowDown": {
        e.preventDefault()
        focusAt(idx < 0 ? 0 : idx + 1)

        break
      }
      case "ArrowLeft":
      case "ArrowUp": {
        e.preventDefault()
        focusAt(idx < 0 ? 0 : idx - 1)

        break
      }
      case "Enter": {
        e.preventDefault()
        // macOS Finder convention: Enter renames the focused entry (double-click
        // opens it). Begins the inline editor on the current selection.
        if (d.focusPath) {
          const n = d.children.find((c) => c.path === d.focusPath)
          if (n) d.startRename(n)
        }

        break
      }
      default: {
        if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "o") {
          // …and Cmd/Ctrl+O opens, preserving a keyboard path to open now that
          // Enter is bound to rename.
          e.preventDefault()
          if (d.focusPath) {
            const n = d.children.find((c) => c.path === d.focusPath)
            if (n) d.open(n)
          }
        } else if ((e.metaKey || e.ctrlKey) && e.key === "Backspace") {
          // ⌘⌫ — move the current selection to Trash.
          e.preventDefault()
          const paths = d.selected.size > 0 ? [...d.selected] : d.focusPath ? [d.focusPath] : []
          if (paths.length > 0) d.trashPaths(paths)
        } else if (e.key === "Backspace" || ((e.metaKey || e.ctrlKey) && e.key === "ArrowUp")) {
          e.preventDefault()
          d.goUp()
        } else if (e.key === " ") {
          e.preventDefault()
          d.setPreviewOpen((o) => !o)
        } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
          e.preventDefault()
          d.setSelected(new Set(d.sorted.map((n) => n.path)))
        } else if (e.key === "Escape") {
          if (d.menuOpen) d.setMenu(null)
          else {
            d.setSelected(new Set())
            d.setFocusPath(null)
          }
        } else if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
          // type-ahead
          typeBuf.current += e.key.toLowerCase()
          window.clearTimeout(typeTimer.current)
          typeTimer.current = window.setTimeout(() => (typeBuf.current = ""), 700)
          const hit = d.sorted.find((n) => n.name.toLowerCase().startsWith(typeBuf.current))
          if (hit) {
            d.setFocusPath(hit.path)
            d.setSelected(new Set([hit.path]))
            d.setAnchor(hit.path)
            d.setPreview(hit)
          }
        }
      }
    }
  }
}
