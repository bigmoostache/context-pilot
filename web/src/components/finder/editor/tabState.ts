import { useCallback, useEffect, useMemo, useState } from "react"
import { arrayMove } from "@dnd-kit/sortable"
import type { DragEndEvent } from "@dnd-kit/core"
import type { FinderNode } from "@/lib/types"

/**
 * The open-file model behind the editor — VS Code's, now with SPLIT VIEW
 * (T630 P3). What was a single flat tab list is a list of EDITOR GROUPS laid
 * side by side, each with its own tabs and its own active file, plus one
 * active-group cursor. A file opened from the explorer lands in the active
 * group; a tab can be dragged from one group into another; a group's "split"
 * button clones its current file into a fresh group to the right.
 *
 * THE PREVIEW TAB IS STILL THE POINT, now PER GROUP. A single click opens a
 * file in a TRANSIENT tab (rendered in italics) in the active group, and the
 * next single click there REPLACES that tab rather than adding another. Double
 * click PINS it. At most one preview tab exists PER GROUP — that invariant is
 * what makes "replace" well defined, and it is enforced group-locally.
 *
 * FLAT AND HORIZONTAL, deliberately. VS Code's true model is a recursive tree
 * of groups with arbitrary nesting and vertical splits; this is the flat
 * horizontal subset that covers almost all real use. Nesting, vertical splits
 * and drag-a-tab-to-the-edge-to-split are intentionally out of scope here.
 *
 * Every action is `useCallback`-stable and the returned bundle is memoized, so
 * a consumer can list it in an effect's dependency array without the effect
 * re-firing every render (which is what lets {@link Finder}'s reveal effect
 * satisfy exhaustive-deps with no lint escape).
 */
export interface OpenTab {
  /** Absolute path — the tab's identity within its group, and the key the tree
   *  highlights on. */
  readonly path: string
  readonly name: string
  readonly kind: FinderNode["kind"]
  /**
   * A transient preview tab, shown in italics and replaced by the next
   * single click. False once the user has committed to the file.
   */
  readonly preview: boolean
}

/** One editor group — a strip of tabs with its own active file. */
export interface EditorGroup {
  /** Stable group identity, used in drag ids and as the React key. */
  readonly id: string
  readonly tabs: readonly OpenTab[]
  /** Absolute path of this group's active tab, or null when it holds none. */
  readonly activePath: string | null
}

export interface GroupsState {
  readonly groups: readonly EditorGroup[]
  /** The group the explorer opens into, and the one drawn as focused. */
  readonly activeGroupId: string
  /** The active GROUP's active path — the single value the explorer highlights
   *  on and the reveal effect drives. */
  readonly activePath: string | null

  // ── explorer-facing (act on the active group) ──
  /** Single click: open transiently in the active group, replacing its preview. */
  openPreview: (node: FinderNode) => void
  /** Double click (or an edit): open pinned in the active group, or pin the tab
   *  already showing it there. */
  openPinned: (node: FinderNode) => void

  // ── per-group ──
  activate: (groupId: string, path: string) => void
  close: (groupId: string, path: string) => void
  reorder: (groupId: string, fromPath: string, toPath: string) => void
  setActiveGroup: (groupId: string) => void
  /** Clone the active group's current file into a new group to its right. */
  splitActive: () => void

  /** Central dnd dispatch (explorer-open · in-group reorder · cross-group move),
   *  so the shell stays a thin `onDragEnd={groups.applyDragEnd}`. */
  applyDragEnd: (e: DragEndEvent) => void
}

/** A unique group id. A random suffix (not a session-monotonic counter) so ids
 *  restored from localStorage on reload can never collide with ids minted after
 *  it — a counter resets to zero each page load and would re-mint `g1` on the
 *  first split, clashing with a restored `g1`. `crypto.randomUUID` is available
 *  in every target browser and needs no top-level mutable counter. */
const nextGroupId = () => `g${crypto.randomUUID().slice(0, 8)}`

/** The dnd id for a tab, namespaced by its group so the same file open in two
 *  groups still has two distinct draggable identities. */
export const tabDragId = (groupId: string, path: string) => `${groupId}::${path}`
/** The dnd id for a whole group's drop area (a tab released over empty strip). */
export const groupDropId = (groupId: string) => `group:${groupId}`

// ── persistence ──────────────────────────────────────────────────────
//
// The open tabs, splits, and active cursor are a per-agent VIEW preference, not
// business logic — the class of state App already keeps in localStorage (view +
// active agent). Persisting it here is what makes the layout survive both a
// view switch (the Finder unmounts, so its `useState` would otherwise be lost)
// and a full page reload. Keyed per agent: agent A's open files mean nothing in
// agent B's realm.

const STORAGE_PREFIX = "cp:finder:groups:"

interface PersistedGroups {
  readonly groups: readonly EditorGroup[]
  readonly activeGroupId: string
}

/** Read a saved layout for `agentId`, or null when absent/unparseable. A stored
 *  layout with no groups is treated as absent so hydration always yields at
 *  least one group. */
function loadGroups(agentId: string): PersistedGroups | null {
  try {
    const raw = localStorage.getItem(STORAGE_PREFIX + agentId)
    if (raw === null) return null
    const parsed = JSON.parse(raw) as PersistedGroups
    if (!Array.isArray(parsed.groups) || parsed.groups.length === 0) return null
    return parsed
  } catch {
    // A corrupt or unreadable entry must never break the editor — fall back to
    // a fresh empty layout.
    return null
  }
}

/** Best-effort write of the current layout for `agentId`. A full or denied
 *  localStorage is swallowed: persistence is a convenience, not a requirement. */
function saveGroups(agentId: string, value: PersistedGroups): void {
  try {
    localStorage.setItem(STORAGE_PREFIX + agentId, JSON.stringify(value))
  } catch {
    // quota exceeded / storage denied — nothing to do, the layout simply
    // won't survive this session.
  }
}

/** Insert a fresh preview tab into a group, replacing its existing preview in
 *  place (so the strip does not reshuffle under the pointer) or appending. */
function withPreview(group: EditorGroup, node: FinderNode): EditorGroup {
  if (group.tabs.some((t) => t.path === node.path)) {
    return { ...group, activePath: node.path }
  }
  const fresh: OpenTab = { path: node.path, name: node.name, kind: node.kind, preview: true }
  const at = group.tabs.findIndex((t) => t.preview)
  const tabs = at === -1 ? [...group.tabs, fresh] : group.tabs.map((t, i) => (i === at ? fresh : t))
  return { ...group, tabs, activePath: node.path }
}

/** Add or promote a pinned tab in a group. */
function withPinned(group: EditorGroup, node: FinderNode): EditorGroup {
  const at = group.tabs.findIndex((t) => t.path === node.path)
  if (at === -1) {
    const pinned: OpenTab = { path: node.path, name: node.name, kind: node.kind, preview: false }
    return { ...group, tabs: [...group.tabs, pinned], activePath: node.path }
  }
  const tabs = group.tabs.map((t, i) => (i === at ? { ...t, preview: false } : t))
  return { ...group, tabs, activePath: node.path }
}

/** Remove a tab from a group, handing focus right-then-left like VS Code. */
function withoutTab(group: EditorGroup, path: string): EditorGroup {
  const at = group.tabs.findIndex((t) => t.path === path)
  if (at === -1) return group
  const tabs = group.tabs.filter((t) => t.path !== path)
  const activePath =
    group.activePath === path ? (tabs[at]?.path ?? tabs[at - 1]?.path ?? null) : group.activePath
  return { ...group, tabs, activePath }
}

/** Reorder one tab within a group; a no-op when either endpoint is missing or
 *  the drop lands on itself. Pure so the `setGroups(prev => prev.map(...))` call
 *  site stays one callback shallower than an inline `.findIndex` would make it. */
function reorderTabs(group: EditorGroup, fromPath: string, toPath: string): EditorGroup {
  const from = group.tabs.findIndex((t) => t.path === fromPath)
  const to = group.tabs.findIndex((t) => t.path === toPath)
  if (from === -1 || to === -1 || from === to) return group
  return { ...group, tabs: arrayMove([...group.tabs], from, to) }
}

/** Insert `tab` (pinned) into a group ahead of `beforePath`, or append when
 *  absent; if the tab is already present, just focus it. Pure, for the same
 *  callback-depth reason as {@link reorderTabs}. */
function withMovedInto(group: EditorGroup, tab: OpenTab, beforePath: string | null): EditorGroup {
  if (group.tabs.some((t) => t.path === tab.path)) return { ...group, activePath: tab.path }
  const moved: OpenTab = { ...tab, preview: false }
  const idx = beforePath ? group.tabs.findIndex((t) => t.path === beforePath) : -1
  const tabs = [...group.tabs]
  if (idx === -1) tabs.push(moved)
  else tabs.splice(idx, 0, moved)
  return { ...group, tabs, activePath: tab.path }
}

/**
 * @param agentId The owning agent — the persistence key. The consumer MUST
 *   remount this hook when the agent changes (Finder keys its body by
 *   `agent.id`), so `agentId` is stable for the hook's whole life and the
 *   saved layout is hydrated once, at mount, from a clean slate.
 */
export function useEditorGroups(agentId: string): GroupsState {
  const [groups, setGroups] = useState<readonly EditorGroup[]>(
    () => loadGroups(agentId)?.groups ?? [{ id: nextGroupId(), tabs: [], activePath: null }],
  )
  const [activeGroupId, setActiveGroupId] = useState<string>(
    () => loadGroups(agentId)?.activeGroupId ?? groups[0]?.id ?? nextGroupId(),
  )

  // Write-through persist. `agentId` is stable for the mount (keyed remount), so
  // it never races a save under the wrong key; listing it keeps deps honest.
  useEffect(() => {
    saveGroups(agentId, { groups, activeGroupId })
  }, [agentId, groups, activeGroupId])

  /** Map over one group by id, leaving the rest untouched. */
  const mapGroup = useCallback((id: string, fn: (g: EditorGroup) => EditorGroup) => {
    setGroups((prev) => prev.map((g) => (g.id === id ? fn(g) : g)))
  }, [])

  const openPreview = useCallback(
    (node: FinderNode) => {
      setActiveGroupId((gid) => {
        mapGroup(gid, (g) => withPreview(g, node))
        return gid
      })
    },
    [mapGroup],
  )

  const openPinned = useCallback(
    (node: FinderNode) => {
      setActiveGroupId((gid) => {
        mapGroup(gid, (g) => withPinned(g, node))
        return gid
      })
    },
    [mapGroup],
  )

  const activate = useCallback(
    (groupId: string, path: string) => {
      setActiveGroupId(groupId)
      mapGroup(groupId, (g) => ({ ...g, activePath: path }))
    },
    [mapGroup],
  )

  const setActiveGroup = useCallback((groupId: string) => setActiveGroupId(groupId), [])

  // Remove a tab; if that empties a NON-LAST group, drop the group and move the
  // active cursor to a survivor (the last group never disappears, so there is
  // always somewhere to open the next file).
  const close = useCallback((groupId: string, path: string) => {
    setGroups((prev) => {
      const next = prev.map((g) => (g.id === groupId ? withoutTab(g, path) : g))
      const emptied = next.find((g) => g.id === groupId)?.tabs.length === 0
      if (emptied && next.length > 1) {
        const survivors = next.filter((g) => g.id !== groupId)
        setActiveGroupId((cur) => (cur === groupId ? (survivors[0]?.id ?? cur) : cur))
        return survivors
      }
      return next
    })
  }, [])

  const reorder = useCallback((groupId: string, fromPath: string, toPath: string) => {
    setGroups((prev) => prev.map((g) => (g.id === groupId ? reorderTabs(g, fromPath, toPath) : g)))
  }, [])

  // Split = clone the active group's active tab (pinned) into a new group placed
  // immediately after it, and focus the new group — VS Code's "Split Editor".
  const splitActive = useCallback(() => {
    const newId = nextGroupId()
    setGroups((prev) => {
      const at = prev.findIndex((g) => g.id === activeGroupId)
      const src = prev[at]
      const active = src?.tabs.find((t) => t.path === src.activePath)
      // Nothing open to split: still create the group so the button never dead-ends.
      const tabs: OpenTab[] = active ? [{ ...active, preview: false }] : []
      const group: EditorGroup = { id: newId, tabs, activePath: active?.path ?? null }
      const next = [...prev]
      next.splice(at + 1, 0, group)
      return next
    })
    setActiveGroupId(newId)
  }, [activeGroupId])

  // Move a tab OUT of one group and INTO another, dropping the source group if
  // it empties (and it is not the last one). `beforePath` places it ahead of a
  // target tab; absent, it appends.
  const moveTab = useCallback(
    (fromGroup: string, path: string, toGroup: string, beforePath: string | null) => {
      setGroups((prev) => {
        const src = prev.find((g) => g.id === fromGroup)
        const tab = src?.tabs.find((t) => t.path === path)
        if (!tab) return prev

        let next = prev.map((g) => (g.id === toGroup ? withMovedInto(g, tab, beforePath) : g))
        next = next.map((g) => (g.id === fromGroup ? withoutTab(g, path) : g))
        const emptied = next.find((g) => g.id === fromGroup)?.tabs.length === 0
        if (emptied && next.length > 1) next = next.filter((g) => g.id !== fromGroup)
        return next
      })
      setActiveGroupId(toGroup)
    },
    [],
  )

  const applyDragEnd = useCallback(
    (e: DragEndEvent) => {
      const { active, over } = e
      if (!over) return
      const data = active.data.current
      const overId = String(over.id)
      const overGroup = groupIdOf(overId)

      // Explorer file dropped onto a group: open it pinned there (drag = commit).
      if (data?.["type"] === "explorer-file") {
        const target = overGroup ?? activeGroupId
        mapGroup(target, (g) => withPinned(g, data["node"] as FinderNode))
        setActiveGroupId(target)
        return
      }

      // A tab was dragged. Parse its own group + path from the namespaced id.
      const src = parseTabId(String(active.id))
      if (!src || !overGroup) return
      if (src.groupId === overGroup) {
        const toPath = pathOf(overId)
        if (toPath && toPath !== src.path) reorder(src.groupId, src.path, toPath)
      } else {
        moveTab(src.groupId, src.path, overGroup, pathOf(overId))
      }
    },
    [activeGroupId, mapGroup, moveTab, reorder],
  )

  const activePath = groups.find((g) => g.id === activeGroupId)?.activePath ?? null

  return useMemo(
    () => ({
      groups,
      activeGroupId,
      activePath,
      openPreview,
      openPinned,
      activate,
      close,
      reorder,
      setActiveGroup,
      splitActive,
      applyDragEnd,
    }),
    [
      groups,
      activeGroupId,
      activePath,
      openPreview,
      openPinned,
      activate,
      close,
      reorder,
      setActiveGroup,
      splitActive,
      applyDragEnd,
    ],
  )
}

// ── dnd id parsing ───────────────────────────────────────────────────
//
// Tab ids are `${groupId}::${path}`; group drop zones are `group:${groupId}`.
// A path can itself contain no `::` (POSIX paths don't), so a single split on
// the first `::` is unambiguous.

/** The group a drag ended over, whether it landed on a tab or on empty strip. */
function groupIdOf(overId: string): string | null {
  if (overId.startsWith("group:")) return overId.slice("group:".length)
  return parseTabId(overId)?.groupId ?? null
}

/** The path a drag ended over, or null when it landed on a group's empty area. */
function pathOf(overId: string): string | null {
  return parseTabId(overId)?.path ?? null
}

/** Split a `${groupId}::${path}` tab id back into its parts. */
function parseTabId(id: string): { groupId: string; path: string } | null {
  const at = id.indexOf("::")
  if (at === -1) return null
  return { groupId: id.slice(0, at), path: id.slice(at + 2) }
}
