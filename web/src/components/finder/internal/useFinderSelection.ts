import type { RefObject } from "react"
import type { Agent, FinderNode, FinderSortKey, FinderViewMode } from "@/lib/types"
import { CLICK_SETTLE_MS, req, type Tab } from "./helpers"
import type { MenuPos } from "../ContextMenu"

let tabSeq = 1

interface SelectionDeps {
  agent: Agent
  sorted: FinderNode[]
  anchor: string | null
  selected: Set<string>
  focusPath: string | null
  viewMode: FinderViewMode
  tabs: Tab[]
  crumbs: FinderNode[]
  sortKey: FinderSortKey
  activeId: string
  clickTimer: RefObject<number | undefined>
  setSelected: React.Dispatch<React.SetStateAction<Set<string>>>
  setAnchor: (p: string | null) => void
  setFocusPath: (p: string | null) => void
  setPreview: (n: FinderNode | null) => void
  setPreviewOpen: (on: boolean) => void
  setMenu: (m: MenuPos | null) => void
  setTabs: React.Dispatch<React.SetStateAction<Tab[]>>
  setActiveId: (id: string) => void
  setSortKey: (k: FinderSortKey) => void
  setAsc: (fn: (a: boolean) => boolean) => void
  startRename: (node: FinderNode) => void
  navigate: (path: string) => void
  trashPaths: (paths: string[]) => void
}

/**
 * The Finder's render-coupled selection, open, tab, and context-menu handlers —
 * everything that reads/writes the live selection + tab state in response to a
 * pointer interaction. Split from the component so the file stays under the
 * size limit; the logic is unchanged and threads the component's state setters
 * in via {@link SelectionDeps}.
 *
 * macOS click semantics live here: {@link onRowClick} applies selection
 * INSTANTLY (never reflowing) then defers — by {@link CLICK_SETTLE_MS} — either
 * slow-second-click-to-rename or opening the (non-reflowing Sheet) Quick Look,
 * so a double-click can cancel them first.
 */
export function useFinderSelection(d: SelectionDeps) {
  // Apply selection + focus + preview-target for a node. INSTANT and reflow-free
  // (it never opens the Quick Look pane), so selection feedback is immediate
  // while the layout-affecting open is deferred to onRowClick's timer.
  const select = (node: FinderNode, { additive, range }: { additive: boolean; range: boolean }) => {
    if (range && d.anchor) {
      const ai = d.sorted.findIndex((n) => n.path === d.anchor)
      const bi = d.sorted.findIndex((n) => n.path === node.path)
      if (ai >= 0 && bi >= 0) {
        const [lo, hi] = ai < bi ? [ai, bi] : [bi, ai]
        d.setSelected(new Set(d.sorted.slice(lo, hi + 1).map((n) => n.path)))
      }
    } else if (additive) {
      d.setSelected((cur) => {
        const next = new Set(cur)
        if (next.has(node.path)) next.delete(node.path)
        else next.add(node.path)
        return next
      })
      d.setAnchor(node.path)
    } else {
      d.setSelected(new Set([node.path]))
      d.setAnchor(node.path)
    }
    d.setFocusPath(node.path)
    d.setPreview(node)
  }

  // Row click with macOS-style settle semantics. Selection feedback is INSTANT
  // (never reflows the layout). After a short settle window a click either arms
  // slow-second-click-to-rename (re-click of a sole-selected item) or opens the
  // Quick Look pane (first click of a fresh item). The pane is a non-reflowing
  // Sheet drawer, so opening it never shifts items — which is why click-to-
  // preview, double-click-to-open, and slow-rename coexist in every view.
  const onRowClick = (node: FinderNode, m: { additive: boolean; range: boolean }) => {
    const wasSole =
      !m.additive &&
      !m.range &&
      d.selected.size === 1 &&
      d.selected.has(node.path) &&
      d.focusPath === node.path
    select(node, m)
    window.clearTimeout(d.clickTimer.current)
    if (m.additive || m.range) return
    const inlineCapable = d.viewMode !== "gallery"
    d.clickTimer.current = window.setTimeout(() => {
      if (wasSole && inlineCapable) d.startRename(node)
      else if (!wasSole) d.setPreviewOpen(true)
    }, CLICK_SETTLE_MS)
  }

  /** Open a file in its own tab (reuse an existing tab for the same file). */
  const openInNewTab = (node: FinderNode) => {
    const existing = d.tabs.find((t) => t.fileNode?.path === node.path)
    if (existing) {
      d.setActiveId(existing.id)
      return
    }
    const id = `t${tabSeq++}`
    d.setTabs((ts) => [
      ...ts,
      { id, cwd: node.path, label: node.name, kind: node.kind, fileNode: node, back: [], fwd: [] },
    ])
    d.setActiveId(id)
  }

  const open = (node: FinderNode) => {
    // A double-click / explicit open must pre-empt the deferred single-click
    // effect (no stray preview-open or rename after the item is opened).
    window.clearTimeout(d.clickTimer.current)
    if (node.kind === "folder") d.navigate(node.path)
    else openInNewTab(node)
  }

  const openContext = (e: React.MouseEvent, node: FinderNode) => {
    e.preventDefault()
    // Stop the event reaching the content-area handler below, so a right-click
    // ON an item shows the item menu (not the empty-space menu).
    e.stopPropagation()
    if (!d.selected.has(node.path)) {
      d.setSelected(new Set([node.path]))
      d.setAnchor(node.path)
    }
    d.setFocusPath(node.path)
    d.setPreview(node)
    d.setMenu({ x: e.clientX, y: e.clientY, node })
  }

  // Right-click on the empty content area (not an item) → the realm-level menu
  // (New Folder, Upload, Select All, …). Item handlers stopPropagation, so this
  // only fires for background / gap / padding clicks.
  const openEmptyContext = (e: React.MouseEvent) => {
    e.preventDefault()
    d.setMenu({ x: e.clientX, y: e.clientY })
  }

  const newTab = () => {
    const id = `t${tabSeq++}`
    d.setTabs((ts) => [
      ...ts,
      { id, cwd: d.agent.folder, label: d.agent.name, kind: "folder", back: [], fwd: [] },
    ])
    d.setActiveId(id)
  }
  const closeTab = (id: string) => {
    d.setTabs((ts) => {
      const next = ts.filter((t) => t.id !== id)
      if (id === d.activeId && next.length) d.setActiveId(req(next, -1).id)
      return next.length ? next : ts
    })
  }

  const onSort = (k: FinderSortKey) => {
    if (k === d.sortKey) d.setAsc((a) => !a)
    else {
      d.setSortKey(k)
      d.setAsc(() => true)
    }
  }

  const goUp = () => {
    if (d.crumbs.length > 1) d.navigate(req(d.crumbs, -2).path)
  }

  // Trash a node, expanding to the whole selection when the node is selected.
  const trashNode = (node: FinderNode) =>
    d.trashPaths(d.selected.has(node.path) && d.selected.size > 0 ? [...d.selected] : [node.path])

  return {
    select,
    onRowClick,
    openInNewTab,
    open,
    openContext,
    openEmptyContext,
    newTab,
    closeTab,
    onSort,
    goUp,
    trashNode,
  }
}
