import { useMemo, useState } from "react"
import { UploadCloud } from "lucide-react"
import type {
  Agent,
  FinderNode,
  FinderSortKey,
  FinderViewMode,
} from "@/lib/types"
import { buildRealm, fmtBytes, findNode, pathChain, sortNodes } from "@/lib/finderFs"
import { FinderTabs, FinderToolbar, FinderSidebar, type FinderTab } from "./FinderChrome"
import { GridView, ListView, ColumnsView } from "./FinderViews"
import { FinderPreview } from "./FinderPreview"

interface Tab extends FinderTab {
  back: string[]
  fwd: string[]
}

let tabSeq = 1

/**
 * Finder — a per-agent file manager confined to the agent's realm. Tabs +
 * history navigation, grid / list / Miller-column views, search, sort, a
 * QuickLook preview pane, and a drag-and-drop upload affordance. Design-only:
 * transfers are decorative, the filesystem is mock.
 */
export function Finder({ agent }: { agent: Agent }) {
  const root = useMemo(() => buildRealm(agent.folder, agent.name), [agent])

  const [tabs, setTabs] = useState<Tab[]>(() => [
    { id: "t0", cwd: root.path, label: root.name, back: [], fwd: [] },
  ])
  const [activeId, setActiveId] = useState("t0")
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [preview, setPreview] = useState<FinderNode | null>(null)
  const [previewOpen, setPreviewOpen] = useState(true)
  const [viewMode, setViewMode] = useState<FinderViewMode>("grid")
  const [query, setQuery] = useState("")
  const [sortKey, setSortKey] = useState<FinderSortKey>("name")
  const [asc, setAsc] = useState(true)
  const [dragging, setDragging] = useState(false)
  const [toast, setToast] = useState<string | null>(null)

  const active = tabs.find((t) => t.id === activeId) ?? tabs[0]
  const cwd = active.cwd
  const cwdNode = findNode(root, cwd) ?? root
  const children = cwdNode.children ?? []
  const filtered = query
    ? children.filter((c) => c.name.toLowerCase().includes(query.toLowerCase()))
    : children
  const sorted = sortNodes(filtered, sortKey, asc)
  const crumbs = pathChain(root, cwd)

  // ── mutators ────────────────────────────────────────────────────
  const patchTab = (fn: (t: Tab) => Tab) =>
    setTabs((ts) => ts.map((t) => (t.id === activeId ? fn(t) : t)))

  const flash = (msg: string) => {
    setToast(msg)
    window.setTimeout(() => setToast(null), 2200)
  }

  const navigate = (path: string) => {
    if (path === cwd) return
    patchTab((t) => ({
      ...t,
      back: [...t.back, t.cwd],
      fwd: [],
      cwd: path,
      label: findNode(root, path)?.name ?? t.label,
    }))
    setSelected(new Set())
    setQuery("")
  }
  const back = () =>
    patchTab((t) =>
      t.back.length
        ? { ...t, fwd: [t.cwd, ...t.fwd], cwd: t.back[t.back.length - 1], back: t.back.slice(0, -1), label: findNode(root, t.back[t.back.length - 1])?.name ?? t.label }
        : t,
    )
  const forward = () =>
    patchTab((t) =>
      t.fwd.length
        ? { ...t, back: [...t.back, t.cwd], cwd: t.fwd[0], fwd: t.fwd.slice(1), label: findNode(root, t.fwd[0])?.name ?? t.label }
        : t,
    )

  const onClick = (node: FinderNode, additive: boolean) => {
    setSelected((cur) => {
      if (additive) {
        const next = new Set(cur)
        next.has(node.path) ? next.delete(node.path) : next.add(node.path)
        return next
      }
      return new Set([node.path])
    })
    setPreview(node)
    if (!previewOpen) setPreviewOpen(true)
  }
  const onOpen = (node: FinderNode) => {
    if (node.kind === "folder") navigate(node.path)
    else {
      setPreview(node)
      setPreviewOpen(true)
    }
  }

  const newTab = () => {
    const id = `t${tabSeq++}`
    setTabs((ts) => [...ts, { id, cwd: root.path, label: root.name, back: [], fwd: [] }])
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
    .map((p) => findNode(root, p))
    .reduce((sum, n) => sum + (n?.size ?? 0), 0)

  return (
    <div
      className="relative flex min-w-0 flex-1 flex-col bg-background"
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
        flash("Uploading 3 files to this folder… (design only)")
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
        query={query}
        previewOpen={previewOpen}
        onBack={back}
        onForward={forward}
        onCrumb={navigate}
        onViewMode={setViewMode}
        onQuery={setQuery}
        onUpload={() => flash("Choose files to upload… (design only)")}
        onDownload={() =>
          flash(
            selected.size
              ? `Downloading ${selected.size} item(s)… (design only)`
              : "Select files to download.",
          )
        }
        onTogglePreview={() => setPreviewOpen((o) => !o)}
      />

      <div className="flex min-h-0 flex-1">
        <FinderSidebar root={root} cwd={cwd} onNavigate={navigate} />

        <main
          className={
            viewMode === "columns"
              ? "min-w-0 flex-1 overflow-hidden"
              : "min-w-0 flex-1 overflow-auto"
          }
        >
          {viewMode === "grid" && (
            <GridView nodes={sorted} selected={selected} onClick={onClick} onOpen={onOpen} />
          )}
          {viewMode === "list" && (
            <ListView
              nodes={sorted}
              selected={selected}
              onClick={onClick}
              onOpen={onOpen}
              sortKey={sortKey}
              asc={asc}
              onSort={onSort}
            />
          )}
          {viewMode === "columns" && (
            <ColumnsView
              panes={crumbs.map((c) => ({ path: c.path, nodes: c.children ?? [] }))}
              activePath={new Set(crumbs.map((c) => c.path))}
              selected={selected}
              onClick={onClick}
              onOpen={onOpen}
            />
          )}
        </main>

        {previewOpen && <FinderPreview node={preview} onClose={() => setPreviewOpen(false)} />}
      </div>

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
        <span className="font-mono text-[10.5px]">{cwd}</span>
      </div>

      {/* drag-drop overlay */}
      {dragging && (
        <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center bg-[var(--signal)]/8 backdrop-blur-[2px]">
          <div className="flex flex-col items-center gap-3 rounded-2xl border-2 border-dashed border-[var(--signal)] bg-card/90 px-10 py-8 pop-shadow">
            <UploadCloud className="size-9 text-[var(--signal)]" />
            <span className="text-[14px] font-semibold text-foreground">Drop to upload</span>
            <span className="text-[12px] text-muted-foreground">into {cwdNode.name}</span>
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
