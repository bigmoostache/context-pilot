import type { FinderNode, FinderViewMode } from "@/lib/types"
import {
  useCreateFolder,
  useMoveItems,
  useRenameItem,
  useTrashItems,
  useUploadFiles,
} from "@/lib/live"
import { NEW_FOLDER_SENTINEL, pathName, req, type Tab } from "./helpers"

interface ActionDeps {
  agentId: string
  agentFolder: string
  relCwd: string
  cwd: string
  viewMode: FinderViewMode
  children: FinderNode[]
  hasFileTab: boolean
  flash: (msg: string) => void
  patchTab: (fn: (t: Tab) => Tab) => void
  /** Clear the pending click-settle timer, if any (navigation pre-empts it). */
  clearClickSettle: () => void
  setSelected: (s: Set<string>) => void
  setAnchor: (p: string | null) => void
  setFocusPath: (p: string | null) => void
  setQuery: (q: string) => void
  setRenamingPath: (p: string | null) => void
  setPendingFolderName: (n: string | null) => void
  setViewMode: (m: FinderViewMode) => void
}

/**
 * The Finder's filesystem-mutating actions + path navigation. Owns the live
 * mutation hooks (upload / move / mkdir / rename / trash) and returns the
 * callbacks the component wires into the toolbar, context menu, drag-and-drop,
 * inline editor, and keyboard. Navigation (`navigate`/`back`/`forward`/`goUp`)
 * lives here too because it shares the same flash + tab-patching plumbing.
 */
export function useFinderActions(d: ActionDeps) {
  const upload = useUploadFiles(d.agentId)
  const move = useMoveItems(d.agentId)
  const mkdir = useCreateFolder(d.agentId)
  const rename = useRenameItem(d.agentId)
  const trash = useTrashItems(d.agentId)

  // Upload a set of files into the current directory, then surface them. Used by
  // both the toolbar Upload button (via the hidden file input) and drag-drop.
  const uploadFiles = (files: File[]) => {
    if (files.length === 0) return
    d.flash(`Uploading ${files.length} file${files.length === 1 ? "" : "s"}…`)
    upload.mutate(
      { dir: d.relCwd, files },
      {
        onSuccess: ({ count }) => d.flash(`Uploaded ${count} file${count === 1 ? "" : "s"}.`),
        onError: (err) => d.flash(err instanceof Error ? err.message : "Upload failed"),
      },
    )
  }

  // Begin creating a folder the macOS way: insert an inline-editable placeholder
  // row with a collision-free default name pre-selected. The real mkdir only
  // fires when the user COMMITS the name (commitRename's sentinel branch); Esc
  // or a blank name abandons it without touching disk.
  const newFolder = () => {
    const existing = new Set(d.children.map((c) => c.name.toLowerCase()))
    let name = "untitled folder"
    for (let i = 2; existing.has(name.toLowerCase()); i++) name = `untitled folder ${i}`
    d.setPendingFolderName(name)
    d.setRenamingPath(NEW_FOLDER_SENTINEL)
    // Gallery has no inline name field — fall back to grid so the placeholder is
    // actually editable.
    if (d.viewMode === "gallery") d.setViewMode("grid")
  }

  // Move dragged entries (realm-relative paths) into a destination folder — the
  // Finder's internal drag-and-drop. Both the items and the destination folder
  // path come straight from the backend listing (realm-relative), so the backend
  // confines them directly. Listings refresh on success.
  const moveItemsInto = (paths: string[], destFolder: FinderNode) => {
    if (paths.length === 0) return
    if (paths.includes(destFolder.path)) return // dropped onto itself
    d.flash(`Moving ${paths.length} item${paths.length === 1 ? "" : "s"}…`)
    move.mutate(
      { items: paths, dest: destFolder.path },
      {
        onSuccess: ({ moved }) =>
          d.flash(
            moved > 0
              ? `Moved ${moved} item${moved === 1 ? "" : "s"} to ${destFolder.name}.`
              : "Already there.",
          ),
        onError: (err) => d.flash(err instanceof Error ? err.message : "Move failed"),
      },
    )
    d.setSelected(new Set())
  }

  // Begin inline-renaming an entry (context menu Rename, or Enter on a focused
  // item). Switches the matching name cell to an editable field.
  const startRename = (node: FinderNode) => d.setRenamingPath(node.path)

  // Move entries to the realm trash (right-click "Move to Trash" / ⌘⌫). Trashed
  // entries move into a hidden .cp-trash/ the listing never shows, so they simply
  // vanish from view.
  const trashPaths = (paths: string[]) => {
    if (paths.length === 0) return
    d.flash(`Moving ${paths.length} item${paths.length === 1 ? "" : "s"} to Trash…`)
    trash.mutate(
      { items: paths },
      {
        onSuccess: ({ trashed }) =>
          d.flash(`Moved ${trashed} item${trashed === 1 ? "" : "s"} to Trash.`),
        onError: (err) => d.flash(err instanceof Error ? err.message : "Move to Trash failed"),
      },
    )
    d.setSelected(new Set())
    d.setFocusPath(null)
  }

  // Commit an inline edit. The sentinel placeholder routes to mkdir (CREATE the
  // pending New Folder); any other node routes to rename. A blank or unchanged
  // name is a silent cancel (the field commits on blur even when untouched).
  const commitRename = (node: FinderNode, raw: string) => {
    d.setRenamingPath(null)
    const name = raw.trim()

    // ── New Folder: the sentinel placeholder → real create on commit ──
    if (node.path === NEW_FOLDER_SENTINEL) {
      d.setPendingFolderName(null)
      if (!name) return // abandoned (empty) → no folder created
      d.flash("Creating folder…")
      mkdir.mutate(
        { dir: d.relCwd, name },
        {
          onSuccess: () => d.flash(`Created “${name}”.`),
          onError: (err) => d.flash(err instanceof Error ? err.message : "Could not create folder"),
        },
      )
      return
    }

    // ── Rename an existing entry ──
    if (!name || name === node.name) return
    rename.mutate(
      { path: node.path, name },
      {
        onSuccess: () => d.flash(`Renamed to “${name}”.`),
        onError: (err) => d.flash(err instanceof Error ? err.message : "Rename failed"),
      },
    )
  }

  // Abandon any in-progress inline edit (Esc): a pending New Folder is dropped
  // (never created), an in-progress rename is left untouched.
  const cancelRename = () => {
    d.setRenamingPath(null)
    d.setPendingFolderName(null)
  }

  // The backend lists paths RELATIVE to the realm root; breadcrumbs and `relCwd`
  // expect an absolute, agent.folder-rooted cwd. Normalise every navigation
  // target to absolute so a folder reached by clicking a live listing keeps a
  // valid crumb trail (and Backspace/go-up works).
  const toAbs = (p: string) =>
    p === d.agentFolder || p.startsWith(d.agentFolder + "/") ? p : `${d.agentFolder}/${p}`

  const navigate = (rawPath: string) => {
    d.clearClickSettle()
    const path = toAbs(rawPath)
    if (path === d.cwd && !d.hasFileTab) return
    d.patchTab((t) => ({
      ...t,
      back: [...t.back, t.cwd],
      fwd: [],
      cwd: path,
      kind: "folder",
      fileNode: undefined,
      label: pathName(path),
    }))
    d.setSelected(new Set())
    d.setAnchor(null)
    d.setFocusPath(null)
    d.setQuery("")
  }
  const back = () =>
    d.patchTab((t) =>
      t.back.length > 0
        ? {
            ...t,
            fwd: [t.cwd, ...t.fwd],
            cwd: req(t.back, -1),
            back: t.back.slice(0, -1),
            label: pathName(req(t.back, -1)),
          }
        : t,
    )
  const forward = () =>
    d.patchTab((t) =>
      t.fwd.length > 0
        ? {
            ...t,
            back: [...t.back, t.cwd],
            cwd: req(t.fwd, 0),
            fwd: t.fwd.slice(1),
            label: pathName(req(t.fwd, 0)),
          }
        : t,
    )

  return {
    upload,
    uploadFiles,
    newFolder,
    moveItemsInto,
    startRename,
    trashPaths,
    commitRename,
    cancelRename,
    toAbs,
    navigate,
    back,
    forward,
  }
}
