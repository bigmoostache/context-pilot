import { useEffect, useMemo, useRef, useState } from "react"
import type { Agent, FinderNode, FinderSortKey, FinderViewMode } from "@/lib/types"
import { sortNodes } from "@/lib/support/finderFs"
import { downloadFile, useFs, useFsDescriptions } from "@/lib/live"
import {
  FinderPathBar,
  FinderSidebar,
  FinderTabs,
  FinderToolbar,
} from "./FinderChrome"
import { ColumnsView, GalleryView, GridView, ListView } from "./views/FinderViews"
import { FinderPreview } from "./preview/FinderPreview"
import { type MenuPos } from "./ContextMenu"
import { QuickLookSheet } from "./QuickLookSheet"
import { useMarquee } from "./support/useMarquee"
import { cn } from "@/lib/utils"
import {
  buildCrumbs,
  loadPins,
  NEW_FOLDER_SENTINEL,
  pinsKey,
  type PinnedFolder,
  type Tab,
} from "./internal/helpers"
import { useExternalDragUpload } from "./internal/useExternalDragUpload"
import { useFinderActions } from "./internal/useFinderActions"
import { useFinderKeyboard } from "./internal/useFinderKeyboard"
import { useFinderSelection } from "./internal/useFinderSelection"
import { FinderOverlays } from "./internal/FinderOverlays"

export type { PinnedFolder } from "./internal/helpers"

/**
 * Finder — a per-agent file manager confined to the agent's realm. Tabs +
 * history navigation, four view modes (grid / list / Miller columns / gallery),
 * full keyboard control (arrows, type-ahead, Space QuickLook, Enter rename),
 * range + additive selection, a right-click context menu, an icon-size slider,
 * a QuickLook Sheet drawer, a toggleable path bar, internal drag-and-drop moves,
 * and a drag-and-drop upload affordance. Live data via the orchestration plane.
 *
 * The logic is factored into hooks: filesystem mutations + path navigation in
 * {@link useFinderActions}; render-coupled selection / open / tab / context-menu
 * handlers in {@link useFinderSelection}; the keyboard map in
 * {@link useFinderKeyboard}; the external-file drag-upload lifecycle in
 * {@link useExternalDragUpload}. This file owns view state, derived listings,
 * and the render.
 */
export function Finder({ agent, revealPath, onRevealConsumed }: { agent: Agent; revealPath?: string | null; onRevealConsumed?: () => void }) {
  const root: FinderNode = useMemo(
    () => ({ name: agent.name, path: agent.folder, kind: "folder" as const, modified: "" }),
    [agent],
  )
  const surfaceRef = useRef<HTMLDivElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)


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

  // Live directory listing for the current working directory. The API expects a
  // RELATIVE path (confined_path rejects absolute), so strip the agent's folder
  // prefix before calling useFs.
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

  // Pending single-click settle timer (see CLICK_SETTLE_MS). Cleared by a
  // double-click / open / navigate so those win over the deferred single-click
  // effect.
  const clickTimer = useRef<number | undefined>(undefined)

  const patchTab = (fn: (t: Tab) => Tab) =>
    setTabs((ts) => ts.map((t) => (t.id === activeId ? fn(t) : t)))

  const flash = (msg: string) => {
    setToast(msg)
    window.setTimeout(() => setToast(null), 2200)
  }

  // ── filesystem-mutating actions + path navigation ───────────────
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
  } = useFinderActions({
    agentId: agent.id,
    agentFolder: agent.folder,
    relCwd,
    cwd,
    viewMode,
    children,
    hasFileTab: !!active.fileNode,
    flash,
    patchTab,
    clickTimer,
    setSelected,
    setAnchor,
    setFocusPath,
    setQuery,
    setRenamingPath,
    setPendingFolderName,
    setViewMode,
  })

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
  } = useFinderSelection({
    agent,
    sorted,
    anchor,
    selected,
    focusPath,
    viewMode,
    tabs,
    crumbs,
    sortKey,
    activeId,
    clickTimer,
    setSelected,
    setAnchor,
    setFocusPath,
    setPreview,
    setPreviewOpen,
    setMenu,
    setTabs,
    setActiveId,
    setSortKey,
    setAsc,
    startRename,
    navigate,
    trashPaths,
  })

  // ── external-file drag overlay (window-level lifecycle) ─────────
  useExternalDragUpload(setDragging, uploadFiles)

  const onKeyDown = useFinderKeyboard({
    sorted,
    children,
    focusPath,
    selected,
    menuOpen: !!menu,
    setFocusPath,
    setSelected,
    setAnchor,
    setPreview,
    setPreviewOpen,
    setMenu,
    startRename,
    open,
    goUp,
    trashPaths,
  })

  // focus the surface on mount + when the agent changes, so keys work
  useEffect(() => {
    surfaceRef.current?.focus()
  }, [])

  // Cancel any pending single-click settle timer on unmount.
  useEffect(() => () => window.clearTimeout(clickTimer.current), [])

  // T334: "Show in Finder" — navigate to a file's parent and select it.
  useEffect(() => {
    if (!revealPath) return
    const lastSlash = revealPath.lastIndexOf("/")
    const parentRel = lastSlash >= 0 ? revealPath.slice(0, lastSlash) : ""
    const parentAbs = parentRel ? `${agent.folder}/${parentRel}` : agent.folder
    navigate(parentAbs)
    setSelected(new Set([revealPath]))
    setFocusPath(revealPath)
    onRevealConsumed?.()
  }, [revealPath])

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

            {/* Quick Look — a standard shadcn Sheet drawer from the right edge.
                It is a normal MODAL sheet: the component brings the dimming
                backdrop, slide-in/out animation, focus trap, scroll lock, and
                Esc + click-outside dismissal for free. Only grid & list use it —
                columns has its own trailing Miller preview pane and gallery
                shows the selected item as a hero, so a drawer there would double
                up. The Sheet's built-in close button is hidden because the pane
                renders its own Quick Look header with a Close control. */}
            <QuickLookSheet
              node={previewNode}
              agentId={agent.id}
              open={previewOpen && (viewMode === "grid" || viewMode === "list")}
              onClose={() => setPreviewOpen(false)}
            />
          </div>
        )}
      </div>

      {pathBarOpen && <FinderPathBar crumbs={crumbs} onCrumb={navigate} />}

      <FinderOverlays
        agentId={agent.id}
        relCwd={relCwd}
        itemCount={children.length}
        selected={selected}
        selSize={selSize}
        viewMode={viewMode}
        cwd={cwd}
        sorted={sorted}
        menu={menu}
        dragging={dragging}
        toast={toast}
        flash={flash}
        open={open}
        addPin={addPin}
        startRename={startRename}
        trashNode={trashNode}
        newFolder={newFolder}
        pickFiles={() => fileInputRef.current?.click()}
        setSelected={setSelected}
        setPreviewOpen={setPreviewOpen}
        setMenu={setMenu}
      />
    </div>
  )
}
