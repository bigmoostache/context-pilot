import {
  ArrowLeft,
  ArrowRight,
  ChevronRight,
  Clock,
  Columns3,
  Download,
  FolderPlus,
  GalleryHorizontalEnd,
  LayoutGrid,
  List as ListIcon,
  Maximize2,
  MoreHorizontal,
  PanelBottom,
  Search,
  Share2,
  Sidebar as SidebarIcon,
  Star,
  Upload,
  X,
} from "lucide-react"
import type { FinderKind, FinderNode, FinderTag, FinderViewMode } from "@/lib/types"
import { collectStarred } from "@/lib/finderFs"
import { kindMeta, TAG_META } from "./kind"
import { cn } from "@/lib/utils"

// ── Tab strip ─────────────────────────────────────────────────────
export interface FinderTab {
  id: string
  cwd: string
  label: string
  /** what this tab shows — drives the leading icon (folder, pdf, …) */
  kind: FinderKind
}

export function FinderTabs({
  tabs,
  active,
  onSelect,
  onClose,
  onNew,
}: {
  tabs: FinderTab[]
  active: string
  onSelect: (id: string) => void
  onClose: (id: string) => void
  onNew: () => void
}) {
  return (
    <div className="flex h-9 shrink-0 items-center gap-1 border-b border-border bg-surface px-2">
      {tabs.map((t) => {
        const on = t.id === active
        const TabIcon = kindMeta[t.kind].icon
        return (
          <div
            key={t.id}
            onClick={() => onSelect(t.id)}
            className={cn(
              "group flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-[12px] transition-colors",
              on ? "bg-card text-foreground card-shadow" : "text-muted-foreground hover:bg-muted/60",
            )}
          >
            <TabIcon
              className="size-3.5 shrink-0"
              style={{ color: t.kind === "folder" ? "var(--warn)" : kindMeta[t.kind].accent }}
            />
            <span className="max-w-[130px] truncate">{t.label}</span>
            {tabs.length > 1 && (
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  onClose(t.id)
                }}
                className="flex size-4 items-center justify-center rounded text-muted-foreground/50 opacity-0 transition-opacity hover:text-foreground group-hover:opacity-100"
              >
                <X className="size-3" />
              </button>
            )}
          </div>
        )
      })}
      <button
        onClick={onNew}
        title="New tab"
        className="flex size-6 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted/60 hover:text-foreground"
      >
        +
      </button>
    </div>
  )
}

// ── Main toolbar ──────────────────────────────────────────────────
const VIEW_ORDER: FinderViewMode[] = ["grid", "list", "columns", "gallery"]
const VIEW_ICON = {
  grid: LayoutGrid,
  list: ListIcon,
  columns: Columns3,
  gallery: GalleryHorizontalEnd,
} as const

export function FinderToolbar({
  crumbs,
  canBack,
  canForward,
  viewMode,
  iconSize,
  query,
  previewOpen,
  pathBarOpen,
  onBack,
  onForward,
  onCrumb,
  onViewMode,
  onIconSize,
  onQuery,
  onNewFolder,
  onUpload,
  onDownload,
  onTogglePreview,
  onTogglePathBar,
  fileActive,
  onFileGetInfo,
  onFileDownload,
  onFileShare,
}: {
  crumbs: FinderNode[]
  canBack: boolean
  canForward: boolean
  viewMode: FinderViewMode
  iconSize: number
  query: string
  previewOpen: boolean
  pathBarOpen: boolean
  onBack: () => void
  onForward: () => void
  onCrumb: (path: string) => void
  onViewMode: (m: FinderViewMode) => void
  onIconSize: (n: number) => void
  onQuery: (q: string) => void
  onNewFolder: () => void
  onUpload: () => void
  onDownload: () => void
  onTogglePreview: () => void
  onTogglePathBar: () => void
  /** true when the active tab is a single file (not a folder) */
  fileActive: boolean
  onFileGetInfo: () => void
  onFileDownload: () => void
  onFileShare: () => void
}) {
  const idx = VIEW_ORDER.indexOf(viewMode)
  // collapse a deep breadcrumb: first · … · last two
  const collapsed = crumbs.length > 4
  const shown = collapsed ? [crumbs[0], ...crumbs.slice(-2)] : crumbs

  return (
    <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border bg-surface px-3">
      <div className="flex items-center gap-0.5">
        <NavBtn icon={ArrowLeft} disabled={!canBack} onClick={onBack} title="Back" />
        <NavBtn icon={ArrowRight} disabled={!canForward} onClick={onForward} title="Forward" />
      </div>

      {/* breadcrumb */}
      <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto rounded-lg border border-border bg-card px-2 py-1 card-shadow">
        {shown.map((c, i) => {
          const isFirst = i === 0
          const showEllipsis = collapsed && i === 1
          return (
            <span key={c.path} className="flex shrink-0 items-center">
              {i > 0 && <ChevronRight className="size-3.5 text-muted-foreground/40" />}
              {showEllipsis && (
                <span className="flex items-center">
                  <MoreHorizontal className="size-3.5 text-muted-foreground/50" />
                  <ChevronRight className="size-3.5 text-muted-foreground/40" />
                </span>
              )}
              <button
                onClick={() => onCrumb(c.path)}
                className={cn(
                  "rounded px-1.5 py-0.5 text-[12px] transition-colors hover:bg-muted/70",
                  i === shown.length - 1 ? "font-semibold text-foreground" : "text-muted-foreground",
                )}
              >
                {isFirst ? (
                  <span className="flex items-center gap-1">
                    <kindMeta.folder.icon className="size-3.5" style={{ color: "var(--signal)" }} />
                    {c.name}
                  </span>
                ) : (
                  c.name
                )}
              </button>
            </span>
          )
        })}
      </div>

      {/* icon-size slider (grid only) */}
      {!fileActive && viewMode === "grid" && (
        <div className="flex items-center gap-1.5 rounded-lg border border-border bg-card px-2 py-1 card-shadow">
          <LayoutGrid className="size-3 text-muted-foreground/60" />
          <input
            type="range"
            min={36}
            max={88}
            step={4}
            value={iconSize}
            onChange={(e) => onIconSize(Number(e.target.value))}
            className="finder-slider h-1 w-16 cursor-pointer"
            title="Icon size"
          />
        </div>
      )}

      {/* segmented view switch with sliding indicator */}
      {!fileActive && (
        <div className="relative flex items-center rounded-lg border border-border bg-muted/60 p-0.5">
          <span
            className="absolute inset-y-0.5 left-0.5 w-7 rounded-md bg-card card-shadow transition-transform duration-200"
            style={{ transform: `translateX(${idx * 28}px)` }}
          />
          {VIEW_ORDER.map((m) => {
            const Icon = VIEW_ICON[m]
            return (
              <button
                key={m}
                title={m[0].toUpperCase() + m.slice(1)}
                onClick={() => onViewMode(m)}
                className={cn(
                  "relative z-[1] flex size-7 items-center justify-center rounded-md transition-colors",
                  viewMode === m ? "text-[var(--signal)]" : "text-muted-foreground hover:text-foreground",
                )}
              >
                <Icon className="size-4" />
              </button>
            )
          })}
        </div>
      )}

      {/* search */}
      {!fileActive && (
        <div className="flex h-8 w-[156px] items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 card-shadow focus-within:border-[var(--signal)]/60">
          <Search className="size-3.5 shrink-0 text-muted-foreground/60" />
          <input
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            placeholder="Search realm"
            className="w-full bg-transparent text-[12px] text-foreground outline-none placeholder:text-muted-foreground/50"
          />
          {query && (
            <button onClick={() => onQuery("")} className="text-muted-foreground/50 hover:text-foreground">
              <X className="size-3.5" />
            </button>
          )}
        </div>
      )}

      <div className="mx-0.5 h-5 w-px bg-border" />

      {fileActive ? (
        <>
          <NavBtn icon={Maximize2} onClick={onFileGetInfo} title="Get Info" />
          <NavBtn icon={Download} onClick={onFileDownload} title="Download" />
          <NavBtn icon={Share2} onClick={onFileShare} title="Share" />
        </>
      ) : (
        <>
          <NavBtn icon={FolderPlus} onClick={onNewFolder} title="New folder" />
          <NavBtn icon={Upload} onClick={onUpload} title="Upload" />
          <NavBtn icon={Download} onClick={onDownload} title="Download" />
          <SegBtn icon={PanelBottom} on={pathBarOpen} onClick={onTogglePathBar} title="Path bar" />
          <SegBtn icon={SidebarIcon} on={previewOpen} onClick={onTogglePreview} title="Quick Look pane" />
        </>
      )}
    </div>
  )
}

function NavBtn({
  icon: Icon,
  disabled,
  onClick,
  title,
}: {
  icon: typeof ArrowLeft
  disabled?: boolean
  onClick: () => void
  title: string
}) {
  return (
    <button
      title={title}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex size-8 items-center justify-center rounded-md transition-colors",
        disabled
          ? "cursor-not-allowed text-muted-foreground/30"
          : "text-muted-foreground/80 hover:bg-muted/70 hover:text-foreground",
      )}
    >
      <Icon className="size-4" />
    </button>
  )
}

function SegBtn({
  icon: Icon,
  on,
  onClick,
  title,
}: {
  icon: typeof LayoutGrid
  on: boolean
  onClick: () => void
  title: string
}) {
  return (
    <button
      title={title}
      onClick={onClick}
      className={cn(
        "flex size-8 items-center justify-center rounded-md transition-colors",
        on ? "bg-muted text-[var(--signal)]" : "text-muted-foreground hover:bg-muted/70 hover:text-foreground",
      )}
    >
      <Icon className="size-4" />
    </button>
  )
}

// ── Left sidebar: favorites / locations / tags ────────────────────
const SIDEBAR_TAGS: FinderTag[] = ["red", "orange", "green", "blue", "purple"]

export function FinderSidebar({
  root,
  cwd,
  onNavigate,
  onOpen,
}: {
  root: FinderNode
  cwd: string
  onNavigate: (path: string) => void
  onOpen: (node: FinderNode) => void
}) {
  const topFolders = (root.children ?? []).filter((c) => c.kind === "folder")
  const starred = collectStarred(root)

  return (
    <aside className="flex w-[204px] shrink-0 flex-col gap-3.5 overflow-y-auto border-r border-border bg-surface px-2.5 py-3">
      {starred.length > 0 && (
        <Group label="Favorites">
          {starred.map((n) => {
            const I = kindMeta[n.kind].icon
            return (
              <Place
                key={n.path}
                icon={I}
                label={n.name}
                accent={n.kind === "folder" ? "var(--warn)" : kindMeta[n.kind].accent}
                onClick={() => (n.kind === "folder" ? onNavigate(n.path) : onOpen(n))}
              />
            )
          })}
        </Group>
      )}

      <Group label="Locations">
        <Place
          icon={kindMeta.folder.icon}
          label={root.name}
          active={cwd === root.path}
          accent="var(--signal)"
          onClick={() => onNavigate(root.path)}
        />
        {topFolders.map((f) => (
          <Place
            key={f.path}
            icon={kindMeta.folder.icon}
            label={f.name}
            active={cwd === f.path}
            accent="var(--warn)"
            indent
            onClick={() => onNavigate(f.path)}
          />
        ))}
      </Group>

      <Group label="Tags">
        {SIDEBAR_TAGS.map((t) => (
          <button
            key={t}
            className="flex items-center gap-2.5 rounded-md py-1.5 pl-2 pr-2 text-left text-[12.5px] text-foreground/75 transition-colors hover:bg-muted/60"
          >
            <span
              className="size-2.5 shrink-0 rounded-full ring-1 ring-inset ring-black/10"
              style={{ background: TAG_META[t].color }}
            />
            {TAG_META[t].label}
          </button>
        ))}
      </Group>

      <Group label="Recents">
        <Place icon={Star} label="Starred" accent="var(--interactive)" muted />
        <Place icon={Clock} label="Last 7 days" accent="var(--muted-foreground)" muted />
      </Group>

      <div className="mt-auto rounded-lg border border-dashed border-border px-2.5 py-2 text-[10.5px] leading-relaxed text-muted-foreground/60">
        <span className="flex items-center gap-1 font-medium text-muted-foreground/80">🔒 Confined realm</span>
        This Finder can't navigate above the agent's folder.
      </div>
    </aside>
  )
}

function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="px-2 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
        {label}
      </span>
      {children}
    </div>
  )
}

function Place({
  icon: Icon,
  label,
  active,
  accent,
  indent,
  muted,
  onClick,
}: {
  icon: typeof Star
  label: string
  active?: boolean
  accent: string
  indent?: boolean
  muted?: boolean
  onClick?: () => void
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-2 rounded-md py-1.5 pr-2 text-left text-[12.5px] transition-colors",
        indent ? "pl-5" : "pl-2",
        active ? "bg-card font-medium text-foreground card-shadow" : "text-foreground/75 hover:bg-muted/60",
        muted && "cursor-default opacity-70",
      )}
    >
      <Icon className="size-4 shrink-0" style={{ color: accent }} />
      <span className="truncate">{label}</span>
    </button>
  )
}

// ── Path bar (bottom) ─────────────────────────────────────────────
export function FinderPathBar({
  crumbs,
  onCrumb,
}: {
  crumbs: FinderNode[]
  onCrumb: (path: string) => void
}) {
  return (
    <div className="flex h-7 shrink-0 items-center gap-0.5 overflow-x-auto border-t border-border bg-surface/80 px-3 text-[11px]">
      {crumbs.map((c, i) => (
        <span key={c.path} className="flex shrink-0 items-center">
          {i > 0 && <ChevronRight className="size-3 text-muted-foreground/40" />}
          <button
            onClick={() => onCrumb(c.path)}
            className="flex items-center gap-1 rounded px-1 py-0.5 text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
          >
            <kindMeta.folder.icon className="size-3" style={{ color: "var(--warn)" }} />
            {c.name}
          </button>
        </span>
      ))}
    </div>
  )
}
