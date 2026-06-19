import { useEffect, useMemo, useRef, useState } from "react"
import { UploadCloud } from "lucide-react"
import type {
  Agent,
  FinderNode,
  FinderSortKey,
  FinderViewMode,
} from "@/lib/types"
import { fmtBytes, sortNodes } from "@/lib/finderFs"
import { downloadFile, useCreateFolder, useFs, useFsDescriptions, useMoveItems, useRenameItem, useTrashItems, useUploadFiles } from "@/lib/live"
import { useQueryClient } from "@tanstack/react-query"
import { qk } from "@/lib/sync"

/** Build breadcrumbs from a path relative to the agent's folder. */
function buildCrumbs(
  agentFolder: string,
  agentName: string,
  cwd: string,
): FinderNode[] {
  if (cwd === agentFolder)
    return [{ name: agentName, path: agentFolder, kind: "folder", modified: "" }]
  const rel = cwd.startsWith(agentFolder + "/")
    ? cwd.slice(agentFolder.length + 1)
    : ""
  const parts = rel.split("/").filter(Boolean)
  const crumbs: FinderNode[] = [
    { name: agentName, path: agentFolder, kind: "folder", modified: "" },
  ]
  let cur = agentFolder
  for (const part of parts) {
    cur = `${cur}/${part}`
    crumbs.push({ name: part, path: cur, kind: "folder", modified: "" })
  }
  return crumbs
}

/** Extract the last segment of a path as a human label. */
function pathName(p: string): string {
  const parts = p.split("/")
  return parts[parts.length - 1] || p
}
import {
  FinderPathBar,
  FinderSidebar,
  FinderTabs,
  FinderToolbar,
  type FinderTab,
} from "./FinderChrome"
import { ColumnsView, GalleryView, GridView, ListView } from "./FinderViews"
import { FinderPreview } from "./FinderPreview"
import { ContextMenu, type MenuPos } from "./ContextMenu"
import { Sheet, SheetContent } from "@/components/ui/sheet"
import { useMarquee } from "./useMarquee"
import { cn } from "@/lib/utils"

interface Tab extends FinderTab {
  back: string[]
  fwd: string[]
  /** when set, this is a file tab showing one file instead of a folder */
  fileNode?: FinderNode
}

/** A folder the user has pinned to the sidebar (persisted in localStorage). */
export interface PinnedFolder {
  name: string
  path: string
}

const pinsKey = (agentId: string) => `cp-finder-pins:${agentId}`

/** Load an agent's pinned folders from localStorage (best-effort). */
function loadPins(agentId: string): PinnedFolder[] {
  try {
    const raw = localStorage.getItem(pinsKey(agentId))
    return raw ? (JSON.parse(raw) as PinnedFolder[]) : []
  } catch {
    return []
  }
}

let tabSeq = 1

/** Single-click settle window (ms). A click defers its layout-affecting side
 *  effects (open the Quick Look pane, or arm slow-click-to-rename) by this much
 *  so a *double*-click can cancel them first — the first click of a double no
 *  longer opens the preview pane and reflows the grid out from under the second
 *  click. Shorter than a typical OS double-click threshold so a deliberate
 *  single click still feels responsive. */
const CLICK_SETTLE_MS = 250

/** Sentinel `path` for the not-yet-created "New Folder" placeholder row. A NUL
 *  byte can never appear in a real realm path, so this never collides with a
 *  live entry; the inline editor keys off it to route a commit to mkdir (create)
 *  instead of rename. */
const NEW_FOLDER_SENTINEL = "\u0000__cp_new_folder__"

/**
 * Finder — a per-agent file manager confined to the agent's realm. Tabs +
 * history navigation, four view modes (grid / list / Miller columns / gallery),
 * full keyboard control (arrows, type-ahead, Space QuickLook, ⌘I Get Info),
 * range + additive selection, a right-click context menu, an icon-size slider,
 * a QuickLook preview pane, a toggleable path bar, and a drag-and-drop upload
 * affordance. Design-only: transfers are decorative, the filesystem is mock.
 */
export function Finder({ agent }: { agent: Agent }) {
  const root: FinderNode = useMemo(
    () => ({ name: agent.name, path: agent.folder, kind: "folder" as const, modified: "" }),
    [agent],
  )
  const surfaceRef = useRef<HTMLDivElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const upload = useUploadFiles(agent.id)
  const move = useMoveItems(agent.id)
  const mkdir = useCreateFolder(agent.id)
  const rename = useRenameItem(agent.id)
  const trash = useTrashItems(agent.id)
  const qc = useQueryClient()

  const [tabs, setTabs] = useState<Tab[]>(() => [
    { id: "t0", cwd: agent.folder, label: agent.name, kind: "folder", back: [], fwd: [] },
  ])
  const [activeId, setActiveId] = useState("t0")
  const [selected, setSelected] = useState<Set<string>>(new Set())
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
  const [pins, setPins] = useState<PinnedFolder[]>(() => loadPins(agent.id))

  // Persist pins per-agent whenever they change.
  useEffect(() => {
    try {
      localStorage.setItem(pinsKey(agent.id), JSON.stringify(pins))
    } catch {
      /* storage full / unavailable — pins stay in-session only */
    }
  }, [pins, agent.id])

  const addPin = (p: PinnedFolder) =>
    setPins((cur) => (cur.some((x) => x.path === p.path) ? cur : [...cur, p]))
  const removePin = (path: string) => setPins((cur) => cur.filter((x) => x.path !== path))

  const active = tabs.find((t) => t.id === activeId) ?? tabs[0]
  const cwd = active.cwd

  // Live directory listing for the current working directory.
  // The API expects a RELATIVE path (confined_path rejects absolute), so
  // strip the agent's folder prefix before calling useFs.
  const relCwd = cwd.startsWith(agent.folder + "/")
    ? cwd.slice(agent.folder.length + 1)
    : cwd === agent.folder
      ? ""
      : cwd
  const { data: liveChildren } = useFs(agent.id, relCwd)
  const children = liveChildren ?? []
  // The agent's tree descriptions (realm-relative path → text), for the per-node
  // info badge. One fetch per agent (rarely changes); a node shows the ⓘ badge
  // exactly when its path is described.
  const { data: descriptions } = useFsDescriptions(agent.id)
  const filtered = query
    ? children.filter((c) => c.name.toLowerCase().includes(query.toLowerCase()))
    : children
  const sorted = sortNodes(filtered, sortKey, asc)
  // While naming a New Folder, prepend an inline-editable placeholder row (a
  // synthetic folder at the sentinel path) so the user names it IN PLACE before
  // it's created. Folders sort first anyway, so the top is a natural spot.
  const displayNodes = useMemo<FinderNode[]>(
    () =>
      pendingFolderName != null
        ? [
            { name: pendingFolderName, path: NEW_FOLDER_SENTINEL, kind: "folder", modified: "", count: 0 },
            ...sorted,
          ]
        : sorted,
    [pendingFolderName, sorted],
  )
  const crumbs = useMemo(
    () => buildCrumbs(agent.folder, agent.name, cwd),
    [agent.folder, agent.name, cwd],
  )
  const previewNode = preview ?? (focusPath ? children.find((c) => c.path === focusPath) ?? null : null)

  const typeBuf = useRef("")
  const typeTimer = useRef<number | undefined>(undefined)
  // Pending single-click settle timer (see CLICK_SETTLE_MS). Cleared by a
  // double-click / open / navigate so those win over the deferred single-click
  // effect.
  const clickTimer = useRef<number | undefined>(undefined)

  // ── mutators ────────────────────────────────────────────────────
  const patchTab = (fn: (t: Tab) => Tab) =>
    setTabs((ts) => ts.map((t) => (t.id === activeId ? fn(t) : t)))

  const flash = (msg: string) => {
    setToast(msg)
    window.setTimeout(() => setToast(null), 2200)
  }

  // Upload a set of files into the current directory, then surface them. Used by
  // both the toolbar Upload button (via the hidden file input) and drag-drop.
  const uploadFiles = (files: File[]) => {
    if (files.length === 0) return
    flash(`Uploading ${files.length} file${files.length === 1 ? "" : "s"}…`)
    upload.mutate(
      { dir: relCwd, files },
      {
        onSuccess: ({ count }) =>
          flash(`Uploaded ${count} file${count === 1 ? "" : "s"}.`),
        onError: (err) =>
          flash(err instanceof Error ? err.message : "Upload failed"),
      },
    )
  }

  // ── External-file drag overlay (robust lifecycle) ────────────────
  //
  // The "Drop to upload" overlay must appear ONLY while an OS-file drag hovers
  // the window, and — critically — must disappear on EVERY way a drag can end:
  // a real drop, leaving the window, OR a silent cancel (Esc / release outside),
  // which fires no drop and (in some browsers) no element-level dragleave. The
  // element-only handlers used before got stuck on the cancel path.
  //
  // We track the whole lifecycle at the WINDOW level: dragover sets the overlay
  // and refreshes a heartbeat timestamp; an explicit window drop / dragleave-to-
  // outside clears it immediately; and a heartbeat watchdog is the catch-all —
  // dragover fires continuously while a drag is live, so once it stops (any
  // cancel path) the overlay self-clears within a couple of frames. A ref holds
  // the latest uploadFiles closure so the listeners bind once and never go stale.
  const uploadRef = useRef(uploadFiles)
  uploadRef.current = uploadFiles
  const lastDragOverRef = useRef(0)
  useEffect(() => {
    const isFileDrag = (e: DragEvent) => !!e.dataTransfer?.types?.includes("Files")
    const onOver = (e: DragEvent) => {
      if (!isFileDrag(e)) return
      e.preventDefault() // allow the drop
      lastDragOverRef.current = Date.now()
      setDragging(true)
    }
    const onDrop = (e: DragEvent) => {
      if (!isFileDrag(e)) return
      e.preventDefault()
      setDragging(false)
      const files = Array.from(e.dataTransfer?.files ?? [])
      if (files.length) uploadRef.current(files)
    }
    // Fired when the cursor leaves the document for the outside (relatedTarget
    // null) — clear at once rather than waiting on the watchdog.
    const onLeave = (e: DragEvent) => {
      if (e.relatedTarget === null) setDragging(false)
    }
    // Watchdog: a live drag emits dragover continuously; if none has arrived for
    // a short grace window the drag has ended SOMEHOW (drop elsewhere, left the
    // window, or Esc-cancel) → drop the overlay. Generous enough not to flicker
    // while the pointer holds still over the window.
    const watchdog = window.setInterval(() => {
      if (lastDragOverRef.current && Date.now() - lastDragOverRef.current > 250) {
        lastDragOverRef.current = 0
        setDragging(false)
      }
    }, 100)
    const onEnd = () => setDragging(false)
    window.addEventListener("dragover", onOver)
    window.addEventListener("drop", onDrop)
    window.addEventListener("dragleave", onLeave)
    window.addEventListener("dragend", onEnd)
    return () => {
      window.clearInterval(watchdog)
      window.removeEventListener("dragover", onOver)
      window.removeEventListener("drop", onDrop)
      window.removeEventListener("dragleave", onLeave)
      window.removeEventListener("dragend", onEnd)
    }
  }, [])

  // Begin creating a folder the macOS way: insert an inline-editable placeholder
  // row with a collision-free default name pre-selected. The real mkdir only
  // fires when the user COMMITS the name (commitRename's sentinel branch); Esc
  // or a blank name abandons it without touching disk. Used by both the toolbar
  // New Folder button and the empty-space context menu.
  const newFolder = () => {
    const existing = new Set(children.map((c) => c.name.toLowerCase()))
    let name = "untitled folder"
    for (let i = 2; existing.has(name.toLowerCase()); i++) name = `untitled folder ${i}`
    setPendingFolderName(name)
    setRenamingPath(NEW_FOLDER_SENTINEL)
    // Gallery has no inline name field — fall back to grid so the placeholder is
    // actually editable.
    if (viewMode === "gallery") setViewMode("grid")
  }

  // Move dragged entries (realm-relative paths) into a destination folder — the
  // Finder's internal drag-and-drop. Both the items and the destination folder
  // path come straight from the backend listing (realm-relative), so the
  // backend confines them directly. Listings refresh on success.
  const moveItemsInto = (paths: string[], destFolder: FinderNode) => {
    if (paths.length === 0) return
    if (paths.includes(destFolder.path)) return // dropped onto itself
    flash(`Moving ${paths.length} item${paths.length === 1 ? "" : "s"}…`)
    move.mutate(
      { items: paths, dest: destFolder.path },
      {
        onSuccess: ({ moved }) =>
          flash(
            moved > 0
              ? `Moved ${moved} item${moved === 1 ? "" : "s"} to ${destFolder.name}.`
              : "Already there.",
          ),
        onError: (err) => flash(err instanceof Error ? err.message : "Move failed"),
      },
    )
    setSelected(new Set())
  }

  // Begin inline-renaming an entry (context menu Rename, or Enter on a focused
  // item). Switches the matching name cell to an editable field.
  const startRename = (node: FinderNode) => setRenamingPath(node.path)

  // Move entries to the realm trash (right-click "Move to Trash" / ⌘⌫). If the
  // triggering node is part of the current multi-select, the WHOLE selection is
  // trashed; otherwise just that node. Trashed entries move into a hidden
  // .cp-trash/ the listing never shows, so they simply vanish from view.
  const trashPaths = (paths: string[]) => {
    if (paths.length === 0) return
    flash(`Moving ${paths.length} item${paths.length === 1 ? "" : "s"} to Trash…`)
    trash.mutate(
      { items: paths },
      {
        onSuccess: ({ trashed }) =>
          flash(`Moved ${trashed} item${trashed === 1 ? "" : "s"} to Trash.`),
        onError: (err) => flash(err instanceof Error ? err.message : "Move to Trash failed"),
      },
    )
    setSelected(new Set())
    setFocusPath(null)
  }
  // Trash a node, expanding to the whole selection when the node is selected.
  const trashNode = (node: FinderNode) =>
    trashPaths(selected.has(node.path) && selected.size > 0 ? [...selected] : [node.path])

  // Commit an inline edit. The sentinel placeholder routes to mkdir (CREATE the
  // pending New Folder); any other node routes to rename. A blank or unchanged
  // name is a silent cancel (the field commits on blur even when untouched).
  const commitRename = (node: FinderNode, raw: string) => {
    setRenamingPath(null)
    const name = raw.trim()

    // ── New Folder: the sentinel placeholder → real create on commit ──
    if (node.path === NEW_FOLDER_SENTINEL) {
      setPendingFolderName(null)
      if (!name) return // abandoned (empty) → no folder created
      flash("Creating folder…")
      mkdir.mutate(
        { dir: relCwd, name },
        {
          onSuccess: () => flash(`Created “${name}”.`),
          onError: (err) =>
            flash(err instanceof Error ? err.message : "Could not create folder"),
        },
      )
      return
    }

    // ── Rename an existing entry ──
    if (!name || name === node.name) return
    rename.mutate(
      { path: node.path, name },
      {
        onSuccess: () => flash(`Renamed to “${name}”.`),
        onError: (err) =>
          flash(err instanceof Error ? err.message : "Rename failed"),
      },
    )
  }

  // Abandon any in-progress inline edit (Esc): a pending New Folder is dropped
  // (never created), an in-progress rename is left untouched.
  const cancelRename = () => {
    setRenamingPath(null)
    setPendingFolderName(null)
  }

  // The backend lists paths RELATIVE to the realm root (e.g. "crates",
  // "crates/cp-base"); breadcrumbs and `relCwd` expect an absolute,
  // agent.folder-rooted cwd. Normalise every navigation target to absolute so
  // a folder reached by clicking a live listing keeps a valid crumb trail
  // (and Backspace/go-up works). Crumb/sidebar targets are already absolute.
  const toAbs = (p: string) =>
    p === agent.folder || p.startsWith(agent.folder + "/") ? p : `${agent.folder}/${p}`

  const navigate = (rawPath: string) => {
    window.clearTimeout(clickTimer.current)
    const path = toAbs(rawPath)
    if (path === cwd && !active.fileNode) return
    patchTab((t) => ({
      ...t,
      back: [...t.back, t.cwd],
      fwd: [],
      cwd: path,
      kind: "folder",
      fileNode: undefined,
      label: pathName(path),
    }))
    setSelected(new Set())
    setAnchor(null)
    setFocusPath(null)
    setQuery("")
  }
  const back = () =>
    patchTab((t) =>
      t.back.length
        ? { ...t, fwd: [t.cwd, ...t.fwd], cwd: t.back[t.back.length - 1], back: t.back.slice(0, -1), label: pathName(t.back[t.back.length - 1]) }
        : t,
    )
  const forward = () =>
    patchTab((t) =>
      t.fwd.length
        ? { ...t, back: [...t.back, t.cwd], cwd: t.fwd[0], fwd: t.fwd.slice(1), label: pathName(t.fwd[0]) }
        : t,
    )
  const goUp = () => {
    if (crumbs.length > 1) navigate(crumbs[crumbs.length - 2].path)
  }

  // Apply selection + focus + preview-target for a node. INSTANT and
  // reflow-free (it never opens the Quick Look pane), so selection feedback is
  // immediate while the layout-affecting open is deferred to onRowClick's timer.
  const select = (node: FinderNode, { additive, range }: { additive: boolean; range: boolean }) => {
    if (range && anchor) {
      const ai = sorted.findIndex((n) => n.path === anchor)
      const bi = sorted.findIndex((n) => n.path === node.path)
      if (ai >= 0 && bi >= 0) {
        const [lo, hi] = ai < bi ? [ai, bi] : [bi, ai]
        setSelected(new Set(sorted.slice(lo, hi + 1).map((n) => n.path)))
      }
    } else if (additive) {
      setSelected((cur) => {
        const next = new Set(cur)
        if (next.has(node.path)) next.delete(node.path)
        else next.add(node.path)
        return next
      })
      setAnchor(node.path)
    } else {
      setSelected(new Set([node.path]))
      setAnchor(node.path)
    }
    setFocusPath(node.path)
    setPreview(node)
  }

  // Row click with macOS-style settle semantics. Selection feedback is INSTANT
  // (and never reflows the layout). After a short settle window a click either
  // arms the slow-second-click-to-rename gesture (re-click of a sole-selected
  // item) or opens the Quick Look pane (first click of a fresh item).
  //
  // The Quick Look pane is rendered as a NON-REFLOWING OVERLAY (absolutely
  // positioned over the right of the content — see the render below), so
  // opening it never shifts the items in the grid/list/columns. That is the
  // crux: an earlier in-flow pane shrank the content and reflowed the auto-fill
  // grid, so the first click of a double-click moved the item out from under
  // the second click, and a deferred open reflowed mid-slow-rename. With a
  // non-reflowing overlay, click-to-preview, double-click-to-open, and
  // slow-click-to-rename all coexist identically across every view. The settle
  // timer still lets a double-click pre-empt the preview-open so opening a file
  // doesn't first flash the pane.
  const onRowClick = (node: FinderNode, m: { additive: boolean; range: boolean }) => {
    const wasSole =
      !m.additive &&
      !m.range &&
      selected.size === 1 &&
      selected.has(node.path) &&
      focusPath === node.path
    select(node, m)
    window.clearTimeout(clickTimer.current)
    if (m.additive || m.range) return
    const inlineCapable = viewMode !== "gallery"
    // After the settle window (so a double-click / open can pre-empt it), a
    // click does one of two things, depending on whether it landed on the
    // already-sole-selected item:
    //   • re-click a sole-selected item → slow-click-to-rename (macOS gesture);
    //   • first click of a fresh item   → open the Quick Look pane.
    // The Quick Look pane is a NON-REFLOWING overlay (see render), so opening it
    // never shifts items — which is why restoring click-to-preview no longer
    // reintroduces the "my item moves under my second click" problem in any
    // view, and the slow-rename gesture keeps working in grid / list / columns.
    clickTimer.current = window.setTimeout(() => {
      if (wasSole && inlineCapable) startRename(node)
      else if (!wasSole) setPreviewOpen(true)
    }, CLICK_SETTLE_MS)
  }

  /** Open a file in its own tab (reuse an existing tab for the same file). */
  const openInNewTab = (node: FinderNode) => {
    const existing = tabs.find((t) => t.fileNode?.path === node.path)
    if (existing) {
      setActiveId(existing.id)
      return
    }
    const id = `t${tabSeq++}`
    setTabs((ts) => [
      ...ts,
      { id, cwd: node.path, label: node.name, kind: node.kind, fileNode: node, back: [], fwd: [] },
    ])
    setActiveId(id)
  }

  const open = (node: FinderNode) => {
    // A double-click / explicit open must pre-empt the deferred single-click
    // effect (no stray preview-open or rename after the item is opened).
    window.clearTimeout(clickTimer.current)
    if (node.kind === "folder") navigate(node.path)
    else openInNewTab(node)
  }

  const openContext = (e: React.MouseEvent, node: FinderNode) => {
    e.preventDefault()
    // Stop the event reaching the content-area handler below, so a right-click
    // ON an item shows the item menu (not the empty-space menu).
    e.stopPropagation()
    if (!selected.has(node.path)) {
      setSelected(new Set([node.path]))
      setAnchor(node.path)
    }
    setFocusPath(node.path)
    setPreview(node)
    setMenu({ x: e.clientX, y: e.clientY, node })
  }

  // Right-click on the empty content area (not an item) → the realm-level menu
  // (New Folder, Upload, Select All, …). Item handlers stopPropagation, so this
  // only fires for background / gap / padding clicks.
  const openEmptyContext = (e: React.MouseEvent) => {
    e.preventDefault()
    setMenu({ x: e.clientX, y: e.clientY })
  }

  // ── keyboard control ────────────────────────────────────────────
  const onKeyDown = (e: React.KeyboardEvent) => {
    const tag = (e.target as HTMLElement).tagName
    if (tag === "INPUT" || tag === "TEXTAREA") return

    const idx = focusPath ? sorted.findIndex((n) => n.path === focusPath) : -1
    const focusAt = (i: number) => {
      const n = sorted[Math.max(0, Math.min(sorted.length - 1, i))]
      if (!n) return
      setFocusPath(n.path)
      setSelected(new Set([n.path]))
      setAnchor(n.path)
      setPreview(n)
    }

    if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      e.preventDefault()
      focusAt(idx < 0 ? 0 : idx + 1)
    } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      e.preventDefault()
      focusAt(idx < 0 ? 0 : idx - 1)
    } else if (e.key === "Enter") {
      e.preventDefault()
      // macOS Finder convention: Enter renames the focused entry (double-click
      // opens it). Begins the inline editor on the current selection.
      if (focusPath) {
        const n = children.find((c) => c.path === focusPath)
        if (n) startRename(n)
      }
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "o") {
      // …and Cmd/Ctrl+O opens, preserving a keyboard path to open now that
      // Enter is bound to rename.
      e.preventDefault()
      if (focusPath) {
        const n = children.find((c) => c.path === focusPath)
        if (n) open(n)
      }
    } else if ((e.metaKey || e.ctrlKey) && e.key === "Backspace") {
      // ⌘⌫ — move the current selection to Trash.
      e.preventDefault()
      const paths = selected.size
        ? [...selected]
        : focusPath
          ? [focusPath]
          : []
      if (paths.length) trashPaths(paths)
    } else if (e.key === "Backspace" || ((e.metaKey || e.ctrlKey) && e.key === "ArrowUp")) {
      e.preventDefault()
      goUp()
    } else if (e.key === " ") {
      e.preventDefault()
      setPreviewOpen((o) => !o)
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
      e.preventDefault()
      setSelected(new Set(sorted.map((n) => n.path)))
    } else if (e.key === "Escape") {
      if (menu) setMenu(null)
      else {
        setSelected(new Set())
        setFocusPath(null)
      }
    } else if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      // type-ahead
      typeBuf.current += e.key.toLowerCase()
      window.clearTimeout(typeTimer.current)
      typeTimer.current = window.setTimeout(() => (typeBuf.current = ""), 700)
      const hit = sorted.find((n) => n.name.toLowerCase().startsWith(typeBuf.current))
      if (hit) {
        setFocusPath(hit.path)
        setSelected(new Set([hit.path]))
        setAnchor(hit.path)
        setPreview(hit)
      }
    }
  }

  // focus the surface on mount + when the agent changes, so keys work
  useEffect(() => {
    surfaceRef.current?.focus()
  }, [])

  // Cancel any pending single-click settle timer on unmount.
  useEffect(() => () => window.clearTimeout(clickTimer.current), [])

  // ── tabs ────────────────────────────────────────────────────────
  const newTab = () => {
    const id = `t${tabSeq++}`
    setTabs((ts) => [...ts, { id, cwd: agent.folder, label: agent.name, kind: "folder", back: [], fwd: [] }])
    setActiveId(id)
  }
  const closeTab = (id: string) => {
    setTabs((ts) => {
      const next = ts.filter((t) => t.id !== id)
      if (id === activeId && next.length) setActiveId(next[next.length - 1].id)
      return next.length ? next : ts
    })
  }

  const onSort = (k: FinderSortKey) => {
    if (k === sortKey) setAsc((a) => !a)
    else {
      setSortKey(k)
      setAsc(true)
    }
  }

  // ── status bar figures ──────────────────────────────────────────
  const selSize = [...selected]
    .map((p) => children.find((c) => c.path === p))
    .reduce((sum, n) => sum + (n?.size ?? 0), 0)

  const viewProps = {
    selected,
    focusPath,
    onClick: onRowClick,
    onOpen: open,
    onContext: openContext,
    onMove: moveItemsInto,
    renamingPath,
    onRenameCommit: commitRename,
    onRenameCancel: cancelRename,
    descriptions,
  }

  // ── box (marquee) selection ─────────────────────────────────────
  const mainRef = useRef<HTMLElement>(null)
  const marqueeOn = viewMode === "grid" || viewMode === "list"
  const marquee = useMarquee({
    containerRef: mainRef,
    enabled: marqueeOn,
    getSelected: () => selected,
    onChange: setSelected,
    onEmptyClick: () => {
      setSelected(new Set())
      setFocusPath(null)
    },
  })

  return (
    <div
      ref={surfaceRef}
      tabIndex={0}
      onKeyDown={onKeyDown}
      className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-background outline-none"
    >
      <input
        ref={fileInputRef}
        type="file"
        multiple
        hidden
        onChange={(e) => {
          uploadFiles(Array.from(e.target.files ?? []))
          e.target.value = "" // allow re-selecting the same file
        }}
      />
      <FinderTabs
        tabs={tabs}
        active={activeId}
        onSelect={setActiveId}
        onClose={closeTab}
        onNew={newTab}
      />
      <FinderToolbar
        crumbs={crumbs}
        canBack={active.back.length > 0}
        canForward={active.fwd.length > 0}
        viewMode={viewMode}
        iconSize={iconSize}
        query={query}
        previewOpen={previewOpen}
        pathBarOpen={pathBarOpen}
        onBack={back}
        onForward={forward}
        onCrumb={navigate}
        onViewMode={setViewMode}
        onIconSize={setIconSize}
        onQuery={setQuery}
        onNewFolder={newFolder}
        onUpload={() => fileInputRef.current?.click()}
        onDownload={() => {
          if (!selected.size) {
            flash("Select files to download.")
            return
          }
          const files = [...selected]
            .map((p) => children.find((c) => c.path === p))
            .filter((n) => n && n.kind !== "folder")
          if (files.length === 0) {
            flash("Select files (not folders) to download.")
            return
          }
          for (const node of files) {
            if (node) downloadFile(agent.id, node.path).catch(() => flash(`Failed to download ${node.name}`))
          }
          flash(`Downloading ${files.length} file(s)…`)
        }}
        onTogglePreview={() => setPreviewOpen((o) => !o)}
        onTogglePathBar={() => setPathBarOpen((o) => !o)}
        fileActive={!!active.fileNode}
        onFileDownload={() => {
          if (active.fileNode) {
            downloadFile(agent.id, active.fileNode.path).catch(() => flash("Download failed"))
            flash(`Downloading ${active.fileNode.name}…`)
          }
        }}
        onFileShare={() => flash(`Share ${active.fileNode?.name ?? "file"}…`)}
      />

      <div className="flex min-h-0 flex-1">
        {/* The Favorites/Locations/Tags sidebar belongs to folder browsing.
            On a file tab the main area is a full-bleed QuickLook of one file,
            so the explorer sidebar is irrelevant — hide it entirely. */}
        {!active.fileNode && (
          <FinderSidebar
            root={root}
            cwd={cwd}
            pins={pins}
            onNavigate={navigate}
            onOpen={open}
            onPin={(p) => {
              addPin(p)
              flash(`Pinned ${p.name}`)
            }}
            onUnpin={removePin}
          />
        )}

        {active.fileNode ? (
          <FinderPreview
            node={active.fileNode}
            variant="full"
            agentId={agent.id}
            onClose={() => closeTab(active.id)}
          />
        ) : (
          <div className="relative flex min-w-0 flex-1">
            <main
              ref={mainRef}
              {...marquee.handlers}
              onContextMenu={openEmptyContext}
              onClick={(e) => {
                if (marquee.didDrag()) return
                if (e.currentTarget === e.target) {
                  setSelected(new Set())
                  setFocusPath(null)
                }
              }}
              className={cn(
                "relative min-w-0 flex-1",
                viewMode === "columns" || viewMode === "gallery"
                  ? "overflow-hidden"
                  : "overflow-auto",
                marqueeOn && "select-none",
              )}
            >
              {marquee.band && (
                <div
                  className="pointer-events-none absolute z-10 rounded-[2px] border border-[var(--signal)] bg-[var(--signal)]/12"
                  style={{
                    left: marquee.band.left,
                    top: marquee.band.top,
                    width: marquee.band.width,
                    height: marquee.band.height,
                  }}
                />
              )}
              {viewMode === "grid" && (
                <GridView key={cwd} nodes={displayNodes} iconSize={iconSize} {...viewProps} />
              )}
              {viewMode === "list" && (
                <ListView
                  key={cwd}
                  nodes={displayNodes}
                  sortKey={sortKey}
                  asc={asc}
                  onSort={onSort}
                  {...viewProps}
                />
              )}
              {viewMode === "columns" && (
                <ColumnsView
                  agentId={agent.id}
                  agentFolder={agent.folder}
                  chain={crumbs.map((c) => c.path)}
                  currentNodes={displayNodes}
                  previewNode={previewNode}
                  onNavigate={navigate}
                  {...viewProps}
                />
              )}
              {viewMode === "gallery" && (
                <GalleryView key={cwd} nodes={sorted} hero={previewNode} {...viewProps} />
              )}
            </main>

            {/* Quick Look — a shadcn Sheet drawer anchored to the right edge.
                It is NON-MODAL (modal={false}) with no backdrop (showOverlay
                false) and pointer-dismissal disabled, so clicking another file
                behind it never closes it — it just updates the live preview
                (previewNode tracks the selection). Esc and the pane's own Close
                X dismiss it. Fixed-positioned, so it never reflows the grid.
                Only grid & list use it: columns has its own trailing Miller
                preview pane and gallery shows the selected item as a hero, so a
                drawer there would double up. */}
            <Sheet
              open={previewOpen && (viewMode === "grid" || viewMode === "list")}
              onOpenChange={(o) => {
                if (!o) setPreviewOpen(false)
              }}
              modal={false}
              disablePointerDismissal
            >
              <SheetContent
                side="right"
                showCloseButton={false}
                showOverlay={false}
                // Width is set with the SAME `data-[side=right]:` modifier the
                // base SheetContent uses for its `w-3/4` + `sm:max-w-sm`, so
                // tailwind-merge cleanly REPLACES them (a plain `w-[420px]` has a
                // different variant prefix, so merge keeps BOTH and the base
                // `w-3/4`/`max-w-sm` win — capping the drawer NARROWER than the
                // 420px FinderPreview pane inside it, which then overflowed past
                // the right viewport edge: the "part off-screen on the right").
                className="border-l border-border p-0 data-[side=right]:w-[420px] data-[side=right]:max-w-[420px] data-[side=right]:sm:max-w-[420px]"
              >
                <FinderPreview
                  node={previewNode}
                  agentId={agent.id}
                  onClose={() => setPreviewOpen(false)}
                />
              </SheetContent>
            </Sheet>
          </div>
        )}
      </div>

      {pathBarOpen && <FinderPathBar crumbs={crumbs} onCrumb={navigate} />}

      {/* status bar */}
      <div className="flex h-7 shrink-0 items-center gap-3 border-t border-border bg-surface px-4 text-[11px] text-muted-foreground">
        <span>{children.length} items</span>
        {selected.size > 0 && (
          <>
            <span className="h-3 w-px bg-border" />
            <span className="text-foreground/80">
              {selected.size} selected · {fmtBytes(selSize)}
            </span>
          </>
        )}
        <span className="ml-auto capitalize">{viewMode} view</span>
        <span className="h-3 w-px bg-border" />
        <span className="hidden sm:inline">128 GB available</span>
        <span className="h-3 w-px bg-border" />
        <span className="font-mono text-[10.5px]">{cwd}</span>
      </div>

      {/* context menu */}
      {menu && (
        <ContextMenu
          pos={menu}
          onClose={() => setMenu(null)}
          onAction={(label) => flash(label)}
          onOpen={open}
          onDownload={(n) => {
            downloadFile(agent.id, n.path).catch(() => flash(`Failed to download ${n.name}`))
            flash(`Downloading ${n.name}…`)
          }}
          onTag={(_n, tag) => flash(`Tagged as ${tag}`)}
          onPin={(n) => {
            addPin({ name: n.name, path: n.path })
            flash(`Pinned ${n.name}`)
          }}
          onRenameStart={startRename}
          onTrash={trashNode}
          onNewFolder={newFolder}
          onUpload={() => fileInputRef.current?.click()}
          onSelectAll={() => setSelected(new Set(sorted.map((n) => n.path)))}
          onTogglePreview={() => setPreviewOpen((o) => !o)}
          onRefresh={() => {
            void qc.invalidateQueries({ queryKey: qk.fs(agent.id, relCwd) })
            flash("Refreshed")
          }}
        />
      )}

      {/* drag-drop overlay */}
      {dragging && (
        <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center bg-[var(--signal)]/8 backdrop-blur-[2px]">
          <div className="ants flex flex-col items-center gap-3 rounded-2xl bg-card/90 px-10 py-8 pop-shadow">
            <UploadCloud className="size-9 text-[var(--signal)]" />
            <span className="text-[14px] font-semibold text-foreground">Drop to upload</span>
            <span className="text-[12px] text-muted-foreground">into {pathName(cwd)}</span>
          </div>
        </div>
      )}

      {/* transient toast */}
      {toast && (
        <div className="absolute bottom-10 left-1/2 z-30 -translate-x-1/2 rounded-lg border border-border bg-card px-4 py-2 text-[12px] text-foreground/90 pop-shadow">
          {toast}
        </div>
      )}
    </div>
  )
}
