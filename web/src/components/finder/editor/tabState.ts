import { useCallback, useMemo, useState } from "react"
import { arrayMove } from "@dnd-kit/sortable"
import type { FinderNode } from "@/lib/types"

/**
 * The open-file model behind the tab strip — VS Code's, including the part
 * people only notice when it is missing.
 *
 * THE PREVIEW TAB IS THE WHOLE POINT. A single click opens a file in a
 * TRANSIENT tab (rendered in italics), and the next single click REPLACES that
 * tab rather than adding another. Without it, browsing a directory of twenty
 * files leaves twenty tabs and the strip becomes unusable within a minute.
 * Double-clicking PINS the tab, which is how a file survives the next click.
 *
 * At most ONE tab is transient at a time — that invariant is what makes
 * "replace" well defined, and it is enforced in {@link openPreview} by pinning
 * nothing and reusing the single `preview` slot.
 *
 * Every action is `useCallback`-stable and the returned bundle is memoized, so
 * a consumer can list it in an effect's dependency array without the effect
 * re-firing every render (which is what lets {@link Finder}'s reveal effect
 * satisfy exhaustive-deps with no lint escape).
 */
export interface OpenTab {
  /** Absolute path — the tab's identity, and the key the tree highlights on. */
  readonly path: string
  readonly name: string
  readonly kind: FinderNode["kind"]
  /**
   * A transient preview tab, shown in italics and replaced by the next
   * single click. False once the user has committed to the file.
   */
  readonly preview: boolean
}

export interface TabsState {
  readonly tabs: readonly OpenTab[]
  /** Absolute path of the active tab, or null when nothing is open. */
  readonly activePath: string | null
  /** Single click: open transiently, replacing any existing preview tab. */
  openPreview: (node: FinderNode) => void
  /** Double click (or an edit): open pinned, or pin the tab already showing it. */
  openPinned: (node: FinderNode) => void
  close: (path: string) => void
  closeAll: () => void
  activate: (path: string) => void
  /** Drag-reorder: move the tab at `fromPath` to `toPath`'s slot (T630). */
  reorder: (fromPath: string, toPath: string) => void
}

export function useTabsState(): TabsState {
  const [tabs, setTabs] = useState<readonly OpenTab[]>([])
  const [activePath, setActivePath] = useState<string | null>(null)

  const openPreview = useCallback((node: FinderNode) => {
    setActivePath(node.path)
    setTabs((prev) => {
      // Already open — clicking it again just focuses it. Crucially it does NOT
      // demote a pinned tab back to preview: the user pinned it deliberately,
      // and a later single click must not silently make it disposable again.
      if (prev.some((t) => t.path === node.path)) return prev

      const fresh: OpenTab = { path: node.path, name: node.name, kind: node.kind, preview: true }
      const at = prev.findIndex((t) => t.preview)
      // Replace the outgoing preview IN PLACE rather than dropping it and
      // appending: the new tab takes the old one's position, so the strip does
      // not reshuffle under the pointer while arrowing through a folder.
      if (at === -1) return [...prev, fresh]
      return prev.map((t, i) => (i === at ? fresh : t))
    })
  }, [])

  const openPinned = useCallback((node: FinderNode) => {
    setActivePath(node.path)
    setTabs((prev) => {
      const at = prev.findIndex((t) => t.path === node.path)
      if (at === -1) {
        return [...prev, { path: node.path, name: node.name, kind: node.kind, preview: false }]
      }
      // Promote the existing tab. Same position, so a double click on the tab
      // the user is already looking at does not move it.
      return prev.map((t, i) => (i === at ? { ...t, preview: false } : t))
    })
  }, [])

  const close = useCallback((path: string) => {
    setTabs((prev) => {
      const at = prev.findIndex((t) => t.path === path)
      if (at === -1) return prev
      const next = prev.filter((t) => t.path !== path)

      // Closing the ACTIVE tab has to hand focus somewhere, and the choice is
      // not arbitrary: VS Code moves to the tab on the RIGHT, falling back to
      // the left at the end of the strip. Jumping to index 0 instead would
      // teleport the user across the strip every time they close something.
      setActivePath((current) =>
        current === path ? (next[at]?.path ?? next[at - 1]?.path ?? null) : current,
      )
      return next
    })
  }, [])

  const closeAll = useCallback(() => {
    setTabs([])
    setActivePath(null)
  }, [])

  const reorder = useCallback((fromPath: string, toPath: string) => {
    setTabs((prev) => {
      const from = prev.findIndex((t) => t.path === fromPath)
      const to = prev.findIndex((t) => t.path === toPath)
      // Either tab gone (closed mid-drag) or a no-op drop onto itself: leave the
      // order untouched rather than splice a −1 index into a phantom move.
      if (from === -1 || to === -1 || from === to) return prev
      return arrayMove([...prev], from, to)
    })
  }, [])

  return useMemo(
    () => ({
      tabs,
      activePath,
      openPreview,
      openPinned,
      close,
      closeAll,
      activate: setActivePath,
      reorder,
    }),
    [tabs, activePath, openPreview, openPinned, close, closeAll, reorder],
  )
}
