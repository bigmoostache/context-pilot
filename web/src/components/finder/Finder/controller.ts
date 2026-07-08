import type { Agent, FinderNode } from "@/lib/types"
import { useFinderActions } from "../internal/useFinderActions"
import { finderDownloads } from "../internal/useFinderDownloads"
import { useFinderKeyboard } from "../internal/useFinderKeyboard"
import { finderSelection } from "../internal/useFinderSelection"
import { useExternalDragUpload } from "../internal/useExternalDragUpload"
import {
  useFinderMount,
  useRevealPath,
  useFinderMarquee,
  useClickSettle,
  finderSelectionSize,
} from "../internal/useFinderState"
import type { Tab } from "../internal/helpers"
import type { FinderViewState, FinderListing } from "./state"
import type { ViewProps } from "./body"

/** Inputs to {@link useFinderController} — the shell's live view state + listing
 *  plus the resolved active tab and the two DOM refs the actions/mount need. */
export interface ControllerArgs {
  agent: Agent
  vs: FinderViewState
  listing: FinderListing
  active: Tab
  cwd: string
  surfaceRef: React.RefObject<HTMLDivElement | null>
  fileInputRef: React.RefObject<HTMLInputElement | null>
  revealPath: string | null | undefined
  onRevealConsumed: (() => void) | undefined
}

/** The marquee (box-selection) surface bindings. */
export interface Marquee {
  mainRef: React.RefObject<HTMLElement | null>
  marqueeOn: boolean
  band: { left: number; top: number; width: number; height: number } | null
  handlers: React.HTMLAttributes<HTMLDivElement>
}

/** Everything the {@link FinderShell} render needs beyond the raw view state:
 *  the filesystem/navigation actions, the render-coupled interaction handlers,
 *  the download actions, the keyboard map, the marquee bindings and the derived
 *  selection size + spread-ready per-node view props. */
export interface FinderController {
  flash: (msg: string) => void
  uploadFiles: (files: File[]) => void
  newFolder: () => void
  navigate: (path: string) => void
  back: () => void
  forward: () => void
  open: (node: FinderNode) => void
  openEmptyContext: (e: React.MouseEvent) => void
  newTab: () => void
  closeTab: (id: string) => void
  onSort: (key: import("@/lib/types").FinderSortKey) => void
  trashNode: (node: FinderNode) => void
  startRename: (node: FinderNode) => void
  downloadSelected: () => void
  downloadActiveFile: () => void
  onKeyDown: (e: React.KeyboardEvent) => void
  selSize: number
  viewProps: ViewProps
  marquee: Marquee
}

/** The filesystem/navigation actions + download actions returned by
 *  {@link useFinderMutations}. */
export interface FinderMutations {
  uploadFiles: (files: File[]) => void
  newFolder: () => void
  moveItemsInto: (paths: string[], dest: FinderNode) => void
  startRename: (node: FinderNode) => void
  trashPaths: (paths: string[]) => void
  commitRename: (node: FinderNode, name: string) => void
  cancelRename: () => void
  navigate: (path: string) => void
  back: () => void
  forward: () => void
  downloadSelected: () => void
  downloadActiveFile: () => void
}

/**
 * The Finder's filesystem-mutating actions + history navigation + download
 * actions, plus the window-level external-drag upload wiring. Split out of
 * {@link useFinderController} so both hooks stay within the P8 line budget.
 */
function useFinderMutations(args: {
  agent: Agent
  vs: FinderViewState
  listing: FinderListing
  active: Tab
  cwd: string
  flash: (msg: string) => void
  patchTab: (fn: (t: Tab) => Tab) => void
  clearClickSettle: () => void
}): FinderMutations {
  const { agent, vs, listing, active, cwd, flash, patchTab, clearClickSettle } = args
  const { relCwd, children, selected } = { ...listing, selected: vs.selected }

  const actions = useFinderActions({
    agentId: agent.id,
    agentFolder: agent.folder,
    relCwd,
    cwd,
    viewMode: vs.viewMode,
    children,
    hasFileTab: !!active.fileNode,
    flash,
    patchTab,
    clearClickSettle,
    setSelected: vs.setSelected,
    setAnchor: vs.setAnchor,
    setFocusPath: vs.setFocusPath,
    setQuery: vs.setQuery,
    setRenamingPath: vs.setRenamingPath,
    setPendingFolderName: vs.setPendingFolderName,
    setViewMode: vs.setViewMode,
  })

  // External-file drag overlay (window-level lifecycle).
  useExternalDragUpload(vs.setDragging, actions.uploadFiles)

  // Download actions (selected files / the active file tab).
  const { downloadSelected, downloadActiveFile } = finderDownloads({
    agentId: agent.id,
    children,
    selected,
    activeFileNode: active.fileNode ?? null,
    flash,
  })

  return { ...actions, downloadSelected, downloadActiveFile }
}

/**
 * Wire every Finder behaviour hook and derive the render-ready handler surface,
 * extracted from the {@link Finder} shell so the component itself is a thin
 * hook-call + {@link FinderShell} render (each function within the P8
 * line/statement/complexity budgets).
 *
 * Owns: the single-click settle timer, the filesystem-mutating actions +
 * history navigation, the render-coupled selection/open/tab/context-menu
 * handlers, the window-level external-drag upload, the download actions, the
 * keyboard map, mount focus, the T334 reveal-path effect, the marquee selection,
 * and the derived selection size + per-node view props.
 */
export function useFinderController(args: ControllerArgs): FinderController {
  const { agent, vs, listing, active, cwd, surfaceRef } = args
  const { children, descriptions, sorted, crumbs } = listing

  // Single-click settle timer — armed on row click, pre-empted by
  // double-click / open / navigate. Owned by useClickSettle (ref never touches
  // render here); the handler factories receive the arm/clear closures.
  const { armClickSettle, clearClickSettle } = useClickSettle()

  const patchTab = (fn: (t: Tab) => Tab) =>
    vs.setTabs((ts) => ts.map((t) => (t.id === vs.activeId ? fn(t) : t)))

  const flash = (msg: string) => {
    vs.setToast(msg)
    window.setTimeout(() => vs.setToast(null), 2200)
  }

  // ── filesystem actions + navigation + downloads (split sub-hook) ─
  const {
    uploadFiles,
    newFolder,
    moveItemsInto,
    startRename,
    trashPaths,
    commitRename,
    cancelRename,
    navigate,
    back,
    forward,
    downloadSelected,
    downloadActiveFile,
  } = useFinderMutations({ agent, vs, listing, active, cwd, flash, patchTab, clearClickSettle })

  // ── render-coupled selection / open / tab / context-menu handlers ─
  const {
    onRowClick,
    open,
    openContext,
    openEmptyContext,
    newTab,
    closeTab,
    onSort,
    goUp,
    trashNode,
  } = finderSelection({
    agent,
    sorted,
    anchor: vs.anchor,
    selected: vs.selected,
    focusPath: vs.focusPath,
    viewMode: vs.viewMode,
    tabs: vs.tabs,
    crumbs,
    sortKey: vs.sortKey,
    activeId: vs.activeId,
    armClickSettle,
    clearClickSettle,
    setSelected: vs.setSelected,
    setAnchor: vs.setAnchor,
    setFocusPath: vs.setFocusPath,
    setPreview: vs.setPreview,
    setPreviewOpen: vs.setPreviewOpen,
    setMenu: vs.setMenu,
    setTabs: vs.setTabs,
    setActiveId: vs.setActiveId,
    setSortKey: vs.setSortKey,
    setAsc: vs.setAsc,
    startRename,
    navigate,
    trashPaths,
  })

  // ── external-file drag overlay (window-level lifecycle) ─────────
  // (wired inside useFinderMutations above)

  const onKeyDown = useFinderKeyboard({
    sorted,
    children,
    focusPath: vs.focusPath,
    selected: vs.selected,
    menuOpen: !!vs.menu,
    setFocusPath: vs.setFocusPath,
    setSelected: vs.setSelected,
    setAnchor: vs.setAnchor,
    setPreview: vs.setPreview,
    setPreviewOpen: vs.setPreviewOpen,
    setMenu: vs.setMenu,
    startRename,
    open,
    goUp,
    trashPaths,
  })

  // focus the surface on mount
  useFinderMount(surfaceRef)

  // T334 "Show in Finder" — navigate to a revealed file's parent + select it.
  useRevealPath({
    revealPath: args.revealPath,
    agentFolder: agent.folder,
    navigate,
    onRevealConsumed: args.onRevealConsumed,
    setSelected: vs.setSelected,
    setFocusPath: vs.setFocusPath,
  })

  const selSize = finderSelectionSize(vs.selected, children)

  const viewProps: ViewProps = {
    selected: vs.selected,
    focusPath: vs.focusPath,
    onClick: onRowClick,
    onOpen: open,
    onContext: openContext,
    onMove: moveItemsInto,
    renamingPath: vs.renamingPath,
    onRenameCommit: commitRename,
    onRenameCancel: cancelRename,
    descriptions,
  }

  // ── box (marquee) selection ─────────────────────────────────────
  const marquee = useFinderMarquee({
    viewMode: vs.viewMode,
    getSelected: () => vs.selected,
    onChange: vs.setSelected,
    onClear: () => {
      vs.setSelected(new Set())
      vs.setFocusPath(null)
    },
  })

  return {
    flash,
    uploadFiles,
    newFolder,
    navigate,
    back,
    forward,
    open,
    openEmptyContext,
    newTab,
    closeTab,
    onSort,
    trashNode,
    startRename,
    downloadSelected,
    downloadActiveFile,
    onKeyDown,
    selSize,
    viewProps,
    marquee,
  }
}
