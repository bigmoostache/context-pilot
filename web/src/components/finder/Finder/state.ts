import { useMemo, useState } from "react"
import type { Agent, FinderNode, FinderSortKey, FinderViewMode } from "@/lib/types"
import { sortNodes } from "@/lib/support/finderFs"
import { useFs, useFsDescriptions } from "@/lib/live"
import { buildCrumbs, NEW_FOLDER_SENTINEL, type Tab } from "../internal/helpers"
import { type MenuPos } from "../ContextMenu"

/**
 * All of the Finder's view state — one bundle so {@link Finder} destructures a
 * single hook call instead of ~18 `useState` statements (which alone blew the
 * P8 max-statements budget). Groups the current tab set + selection + focus +
 * preview + view-mode/appearance + inline-edit + toast/menu state.
 */
export interface FinderViewState {
  tabs: Tab[]
  setTabs: React.Dispatch<React.SetStateAction<Tab[]>>
  activeId: string
  setActiveId: (id: string) => void
  selected: Set<string>
  setSelected: React.Dispatch<React.SetStateAction<Set<string>>>
  anchor: string | null
  setAnchor: (p: string | null) => void
  focusPath: string | null
  setFocusPath: (p: string | null) => void
  preview: FinderNode | null
  setPreview: (n: FinderNode | null) => void
  previewOpen: boolean
  setPreviewOpen: React.Dispatch<React.SetStateAction<boolean>>
  viewMode: FinderViewMode
  setViewMode: (m: FinderViewMode) => void
  iconSize: number
  setIconSize: React.Dispatch<React.SetStateAction<number>>
  query: string
  setQuery: (q: string) => void
  sortKey: FinderSortKey
  setSortKey: (k: FinderSortKey) => void
  asc: boolean
  setAsc: React.Dispatch<React.SetStateAction<boolean>>
  dragging: boolean
  setDragging: (v: boolean) => void
  toast: string | null
  setToast: (t: string | null) => void
  menu: MenuPos | null
  setMenu: (m: MenuPos | null) => void
  renamingPath: string | null
  setRenamingPath: (p: string | null) => void
  pendingFolderName: string | null
  setPendingFolderName: (n: string | null) => void
  pathBarOpen: boolean
  setPathBarOpen: React.Dispatch<React.SetStateAction<boolean>>
}

/** Seed the Finder's view state, opening on a single folder tab at the realm root. */
export function useFinderViewState(agent: Agent): FinderViewState {
  const [tabs, setTabs] = useState<Tab[]>(() => [
    { id: "t0", cwd: agent.folder, label: agent.name, kind: "folder", back: [], fwd: [] },
  ])
  const [activeId, setActiveId] = useState("t0")
  const [selected, setSelected] = useState<Set<string>>(() => new Set())
  const [anchor, setAnchor] = useState<string | null>(null)
  const [focusPath, setFocusPath] = useState<string | null>(null)
  const [preview, setPreview] = useState<FinderNode | null>(null)
  const [previewOpen, setPreviewOpen] = useState(false)
  const [viewMode, setViewMode] = useState<FinderViewMode>("grid")
  const [iconSize, setIconSize] = useState(52)
  const [query, setQuery] = useState("")
  const [sortKey, setSortKey] = useState<FinderSortKey>("name")
  const [asc, setAsc] = useState(true)
  const [dragging, setDragging] = useState(false)
  const [toast, setToast] = useState<string | null>(null)
  const [menu, setMenu] = useState<MenuPos | null>(null)
  const [renamingPath, setRenamingPath] = useState<string | null>(null)
  // When non-null, a "New Folder" is being named inline: a placeholder folder
  // row is shown (with this default name pre-selected) and the real mkdir only
  // fires when the user commits the name. Null = not creating.
  const [pendingFolderName, setPendingFolderName] = useState<string | null>(null)
  const [pathBarOpen, setPathBarOpen] = useState(false)

  return {
    tabs,
    setTabs,
    activeId,
    setActiveId,
    selected,
    setSelected,
    anchor,
    setAnchor,
    focusPath,
    setFocusPath,
    preview,
    setPreview,
    previewOpen,
    setPreviewOpen,
    viewMode,
    setViewMode,
    iconSize,
    setIconSize,
    query,
    setQuery,
    sortKey,
    setSortKey,
    asc,
    setAsc,
    dragging,
    setDragging,
    toast,
    setToast,
    menu,
    setMenu,
    renamingPath,
    setRenamingPath,
    pendingFolderName,
    setPendingFolderName,
    pathBarOpen,
    setPathBarOpen,
  }
}

/** The live directory listing + everything derived from it. */
export interface FinderListing {
  root: FinderNode
  relCwd: string
  children: FinderNode[]
  descriptions: Record<string, string> | undefined
  sorted: FinderNode[]
  displayNodes: FinderNode[]
  crumbs: FinderNode[]
  previewNode: FinderNode | null
}

/**
 * Fetch the current working directory's live listing and derive everything the
 * render needs from it: the search-filtered + sorted nodes, the display list
 * (with the inline New-Folder placeholder prepended while naming one), the
 * breadcrumb chain, and the resolved preview node. Extracted from {@link Finder}
 * so its body stays within the P8 statement/complexity budgets.
 */
export function useFinderListing(args: {
  agentId: string
  agentFolder: string
  agentName: string
  cwd: string
  query: string
  sortKey: FinderSortKey
  asc: boolean
  pendingFolderName: string | null
  preview: FinderNode | null
  focusPath: string | null
}): FinderListing {
  const { agentId, agentFolder, agentName, cwd, query, sortKey, asc } = args
  const { pendingFolderName, preview, focusPath } = args

  // The API expects a RELATIVE path (confined_path rejects absolute), so strip
  // the agent's folder prefix before calling useFs.
  const relCwd =
    cwd === agentFolder
      ? ""
      : cwd.startsWith(agentFolder + "/")
        ? cwd.slice(agentFolder.length + 1)
        : cwd
  const { data: liveChildren } = useFs(agentId, relCwd)
  const children = liveChildren ?? []
  // The agent's tree descriptions (realm-relative path → text), for the per-node
  // info badge. One fetch per agent; a node shows the ⓘ badge when described.
  const { data: descriptions } = useFsDescriptions(agentId)

  const filtered = query
    ? children.filter((c) => c.name.toLowerCase().includes(query.toLowerCase()))
    : children
  const sorted = sortNodes(filtered, sortKey, asc)
  // While naming a New Folder, prepend an inline-editable placeholder row (a
  // synthetic folder at the sentinel path) so the user names it IN PLACE before
  // it's created. Folders sort first anyway, so the top is a natural spot.
  const displayNodes = useMemo<FinderNode[]>(
    () =>
      pendingFolderName == null
        ? sorted
        : [
            {
              name: pendingFolderName,
              path: NEW_FOLDER_SENTINEL,
              kind: "folder",
              modified: "",
              count: 0,
            },
            ...sorted,
          ],
    [pendingFolderName, sorted],
  )
  const crumbs = buildCrumbs(agentFolder, agentName, cwd)
  const previewNode =
    preview ?? (focusPath ? (children.find((c) => c.path === focusPath) ?? null) : null)
  // The realm root node (Favorites sidebar anchor) — stable per agent.
  const root: FinderNode = { name: agentName, path: agentFolder, kind: "folder", modified: "" }

  return { root, relCwd, children, descriptions, sorted, displayNodes, crumbs, previewNode }
}
