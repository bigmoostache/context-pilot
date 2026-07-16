import type { Agent } from "@/lib/types"
import { clickable } from "@/lib/support/a11y"
import { FinderPathBar, FinderTabs, FinderToolbar } from "../FinderChrome"
import { FinderOverlays } from "../internal/FinderOverlays"
import { useFinderPins } from "../internal/useFinderState"
import type { Tab } from "../internal/helpers"
import { FinderBody } from "./body"
import type { FinderViewState, FinderListing } from "./state"
import type { FinderController } from "./controller"

/**
 * The Finder's full chrome + content render: tab strip, main toolbar, the
 * body region (sidebar + views + Quick Look), the optional path bar, and the
 * overlay layer (context menu, drag hint, status bar, toast). Extracted from
 * {@link Finder} so both the shell component and this render stay within the P8
 * line budget; the shell owns only the hook wiring.
 *
 * Favorites pins live here (a per-agent localStorage hook) rather than in the
 * controller because only this render + the overlay consume them.
 */
export function FinderShell({
  agent,
  vs,
  listing,
  active,
  cwd,
  ctrl,
  surfaceRef,
  fileInputRef,
  disconnected,
  onReconnect,
}: {
  agent: Agent
  vs: FinderViewState
  listing: FinderListing
  active: Tab
  cwd: string
  ctrl: FinderController
  surfaceRef: React.RefObject<HTMLDivElement | null>
  fileInputRef: React.RefObject<HTMLInputElement | null>
  disconnected?: boolean | undefined
  onReconnect?: (() => void) | undefined
}) {
  const { root, children, displayNodes, sorted, crumbs, previewNode, relCwd } = listing
  const { pins, addPin, removePin } = useFinderPins(agent.id)

  return (
    <div
      ref={surfaceRef}
      // The Finder is a custom keyboard-driven widget (arrow nav, type-ahead,
      // Space/Enter/⌘⌫ shortcuts), so it takes focus and owns its key handling.
      // role="application" is the honest ARIA role for such a surface — it tells
      // assistive tech to pass keystrokes through — and satisfies the a11y rules
      // for a focusable element with an onKeyDown (interactive role + tabIndex).
      role="application"
      tabIndex={0}
      onKeyDown={ctrl.onKeyDown}
      className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-background outline-none"
      style={disconnected ? { filter: "blur(3px) grayscale(0.5)", transition: "filter 300ms" } : { transition: "filter 300ms" }}
    >
      {disconnected && (
        <div
          {...clickable(() => onReconnect?.())}
          aria-label="Reconnect agent"
          className="absolute inset-0 z-40 cursor-pointer bg-background/30"
        />
      )}
      <input
        ref={fileInputRef}
        type="file"
        multiple
        hidden
        onChange={(e) => {
          ctrl.uploadFiles([...(e.target.files ?? [])])
          e.target.value = "" // allow re-selecting the same file
        }}
      />
      <FinderTabs
        tabs={vs.tabs}
        active={vs.activeId}
        onSelect={vs.setActiveId}
        onClose={ctrl.closeTab}
        onNew={ctrl.newTab}
      />
      <FinderToolbar
        crumbs={crumbs}
        canBack={active.back.length > 0}
        canForward={active.fwd.length > 0}
        viewMode={vs.viewMode}
        iconSize={vs.iconSize}
        query={vs.query}
        previewOpen={vs.previewOpen}
        pathBarOpen={vs.pathBarOpen}
        onBack={ctrl.back}
        onForward={ctrl.forward}
        onCrumb={ctrl.navigate}
        onViewMode={vs.setViewMode}
        onIconSize={vs.setIconSize}
        onQuery={vs.setQuery}
        onNewFolder={ctrl.newFolder}
        onUpload={() => fileInputRef.current?.click()}
        onDownload={ctrl.downloadSelected}
        onTogglePreview={() => vs.setPreviewOpen((o) => !o)}
        onTogglePathBar={() => vs.setPathBarOpen((o) => !o)}
        fileActive={!!active.fileNode}
        onFileDownload={ctrl.downloadActiveFile}
      />

      <FinderBody
        agentId={agent.id}
        agentFolder={agent.folder}
        root={root}
        cwd={cwd}
        pins={pins}
        fileNode={active.fileNode}
        activeTabId={active.id}
        displayNodes={displayNodes}
        sorted={sorted}
        crumbs={crumbs}
        previewNode={previewNode}
        viewMode={vs.viewMode}
        iconSize={vs.iconSize}
        sortKey={vs.sortKey}
        asc={vs.asc}
        previewOpen={vs.previewOpen}
        marqueeOn={ctrl.marquee.marqueeOn}
        band={ctrl.marquee.band}
        mainRef={ctrl.marquee.mainRef}
        handlers={ctrl.marquee.handlers}
        viewProps={ctrl.viewProps}
        onNavigate={ctrl.navigate}
        onOpen={ctrl.open}
        onSort={ctrl.onSort}
        onEmptyContext={ctrl.openEmptyContext}
        onCloseTab={ctrl.closeTab}
        onPin={(p) => {
          addPin(p)
          ctrl.flash(`Pinned ${p.name}`)
        }}
        onUnpin={removePin}
        onClearSelection={() => {
          vs.setSelected(new Set())
          vs.setFocusPath(null)
        }}
        onClosePreview={() => vs.setPreviewOpen(false)}
      />

      {vs.pathBarOpen && <FinderPathBar crumbs={crumbs} onCrumb={ctrl.navigate} />}

      <FinderOverlays
        agentId={agent.id}
        relCwd={relCwd}
        itemCount={children.length}
        selected={vs.selected}
        selSize={ctrl.selSize}
        viewMode={vs.viewMode}
        cwd={cwd}
        sorted={sorted}
        menu={vs.menu}
        dragging={vs.dragging}
        toast={vs.toast}
        flash={ctrl.flash}
        open={ctrl.open}
        addPin={addPin}
        startRename={ctrl.startRename}
        trashNode={ctrl.trashNode}
        newFolder={ctrl.newFolder}
        pickFiles={() => fileInputRef.current?.click()}
        setSelected={vs.setSelected}
        setPreviewOpen={vs.setPreviewOpen}
        setMenu={vs.setMenu}
      />
    </div>
  )
}
