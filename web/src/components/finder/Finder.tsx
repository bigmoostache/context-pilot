import { useEffect, useMemo, useRef, useState } from "react"
import { UploadCloud } from "lucide-react"
import type {
  Agent,
  FinderNode,
  FinderSortKey,
  FinderViewMode,
} from "@/lib/types"
import { fmtBytes, sortNodes } from "@/lib/finderFs"
import { downloadFile, useFs } from "@/lib/live"

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
  const filtered = query
    ? children.filter((c) => c.name.toLowerCase().includes(query.toLowerCase()))
    : children
  const sorted = sortNodes(filtered, sortKey, asc)
  const crumbs = useMemo(
    () => buildCrumbs(agent.folder, agent.name, cwd),
    [agent.folder, agent.name, cwd],
  )
  const previewNode = preview ?? (focusPath ? children.find((c) => c.path === focusPath) ?? null : null)

  const typeBuf = useRef("")
  const typeTimer = useRef<number | undefined>(undefined)

  // ── mutators ────────────────────────────────────────────────────
  const patchTab = (fn: (t: Tab) => Tab) =>
    setTabs((ts) => ts.map((t) => (t.id === activeId ? fn(t) : t)))

  const flash = (msg: string) => {
    setToast(msg)
    window.setTimeout(() => setToast(null), 2200)
  }

  // The backend lists paths RELATIVE to the realm root (e.g. "crates",
  // "crates/cp-base"); breadcrumbs and `relCwd` expect an absolute,
  // agent.folder-rooted cwd. Normalise every navigation target to absolute so
  // a folder reached by clicking a live listing keeps a valid crumb trail
  // (and Backspace/go-up works). Crumb/sidebar targets are already absolute.
  const toAbs = (p: string) =>
    p === agent.folder || p.startsWith(agent.folder + "/") ? p : `${agent.folder}/${p}`

  const navigate = (rawPath: string) => {
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
    if (!previewOpen && node.kind !== "folder") setPreviewOpen(true)
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
    if (node.kind === "folder") navigate(node.path)
    else openInNewTab(node)
  }

  const openContext = (e: React.MouseEvent, node: FinderNode) => {
    e.preventDefault()
    if (!selected.has(node.path)) {
      setSelected(new Set([node.path]))
      setAnchor(node.path)
    }
    setFocusPath(node.path)
    setPreview(node)
    setMenu({ x: e.clientX, y: e.clientY, node })
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
      if (focusPath) {
        const n = children.find((c) => c.path === focusPath)
        if (n) open(n)
      }
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
    onClick: select,
    onOpen: open,
    onContext: openContext,
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
      onDragOver={(e) => {
        e.preventDefault()
        if (!dragging) setDragging(true)
      }}
      onDragLeave={(e) => {
        if (e.currentTarget === e.target) setDragging(false)
      }}
      onDrop={(e) => {
        e.preventDefault()
        setDragging(false)
        flash("Uploading 3 files to this folder…")
      }}
    >
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
        onNewFolder={() => flash("New Folder created")}
        onUpload={() => flash("Choose files to upload…")}
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
          <>
            <main
              ref={mainRef}
              {...marquee.handlers}
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
                <GridView key={cwd} nodes={sorted} iconSize={iconSize} {...viewProps} />
              )}
              {viewMode === "list" && (
                <ListView
                  key={cwd}
                  nodes={sorted}
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
                  currentNodes={sorted}
                  previewNode={previewNode}
                  onNavigate={navigate}
                  {...viewProps}
                />
              )}
              {viewMode === "gallery" && (
                <GalleryView key={cwd} nodes={sorted} hero={previewNode} {...viewProps} />
              )}
            </main>

            {previewOpen && (
              <FinderPreview
                node={previewNode}
                agentId={agent.id}
                onClose={() => setPreviewOpen(false)}
              />
            )}
          </>
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
