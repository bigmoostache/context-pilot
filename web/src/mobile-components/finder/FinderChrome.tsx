import {
  ArrowLeft,
  ArrowRight,
  ChevronRight,
  Columns3,
  Download,
  FolderPlus,
  GalleryHorizontalEnd,
  LayoutGrid,
  List as ListIcon,
  Search,
  Sidebar as SidebarIcon,
  Upload,
  X,
} from "lucide-react"
import type { FinderKind, FinderNode, FinderViewMode } from "@/lib/types"
import { extOf } from "./support/kind"
import { FileIcon } from "./support/macIcons"
import { cn } from "@/lib/utils"
import { clickable } from "@/lib/support/a11y"

// ── Tab strip ─────────────────────────────────────────────────────
// Mobile twin of `components/finder/FinderChrome` FinderTabs. Same tab model;
// the divergence is touch-shaped: the desktop close ✕ is hover-revealed
// (opacity-0 → group-hover), which a phone can never trigger, so on mobile the
// ✕ is ALWAYS visible and a comfortable 24px tap target. The strip scrolls
// horizontally when tabs overflow the narrow viewport.
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
    <div className="no-scrollbar flex h-11 shrink-0 items-center gap-1 overflow-x-auto border-b border-border bg-surface px-2">
      {tabs.map((t) => {
        const on = t.id === active
        return (
          <div
            key={t.id}
            {...clickable(() => onSelect(t.id))}
            className={cn(
              "flex h-8 shrink-0 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-[13px] transition-colors",
              on
                ? "card-shadow bg-card text-foreground"
                : "text-muted-foreground active:bg-muted/60",
            )}
          >
            <FileIcon kind={t.kind} ext={extOf(t.label)} size={16} className="shrink-0" />
            <span className="max-w-[130px] truncate">{t.label}</span>
            {tabs.length > 1 && (
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  onClose(t.id)
                }}
                // Always visible on touch (no hover to reveal it), 24px tap target.
                className="flex size-6 items-center justify-center rounded-sm text-muted-foreground/60 active:text-foreground"
              >
                <X className="size-3.5" />
              </button>
            )}
          </div>
        )
      })}
      <button
        onClick={onNew}
        aria-label="New tab"
        className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 transition-colors active:bg-muted/60 active:text-foreground"
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

/**
 * Mobile toolbar twin. The desktop packs nav + crumbs + an icon-size slider + a
 * 4-mode segmented switch + search + six action buttons into one h-12 bar —
 * impossible at ~375px. So the mobile bar is a SINGLE horizontally-scrollable
 * row of 36px touch controls (the iOS Safari toolbar pattern): every desktop
 * hover-tooltip (`Tip`) is dropped (touch has no hover; a bare button + title
 * suffices), the grid icon-size slider is dropped (grid auto-sizes, no room),
 * and the path-bar toggle is dropped (the scrollable breadcrumbs already serve
 * that). Back/forward, the crumbs, the 4-mode switch, search, and New-Folder/
 * Upload/Download/Quick-Look remain — the essentials, each a proper tap target.
 */
export function FinderToolbar({
  crumbs,
  canBack,
  canForward,
  viewMode,
  query,
  previewOpen,
  onBack,
  onForward,
  onCrumb,
  onViewMode,
  onQuery,
  onNewFolder,
  onUpload,
  onDownload,
  onTogglePreview,
  fileActive,
  onFileDownload,
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
  onFileDownload: () => void
}) {
  const idx = VIEW_ORDER.indexOf(viewMode)

  return (
    <div className="no-scrollbar flex h-13 shrink-0 items-center gap-2 overflow-x-auto border-b border-border bg-surface px-3">
      <div className="flex shrink-0 items-center gap-0.5">
        <NavBtn icon={ArrowLeft} disabled={!canBack} onClick={onBack} title="Back" />
        <NavBtn icon={ArrowRight} disabled={!canForward} onClick={onForward} title="Forward" />
      </div>

      <ToolbarCrumbs crumbs={crumbs} onCrumb={onCrumb} />

      {/* segmented view switch — 36px cells, sliding indicator */}
      {!fileActive && (
        <div className="relative flex shrink-0 items-center rounded-lg border border-border bg-muted/60 p-0.5">
          <span
            className="card-shadow absolute inset-y-0.5 left-0.5 w-9 rounded-md bg-card transition-transform duration-200"
            style={{ transform: `translateX(${idx * 36}px)` }}
          />
          {VIEW_ORDER.map((m) => {
            const Icon = VIEW_ICON[m]
            return (
              <button
                key={m}
                onClick={() => onViewMode(m)}
                aria-label={m}
                className={cn(
                  "relative z-1 flex size-9 items-center justify-center rounded-md transition-colors",
                  viewMode === m
                    ? "text-(--signal)"
                    : "text-muted-foreground active:text-foreground",
                )}
              >
                <Icon className="size-4" />
              </button>
            )
          })}
        </div>
      )}

      {/* search — 16px text kills iOS focus-zoom, shrinkable but keeps a floor */}
      {!fileActive && (
        <div className="card-shadow flex h-9 w-[150px] shrink-0 items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 focus-within:border-(--signal)/60">
          <Search className="size-4 shrink-0 text-muted-foreground/60" />
          <input
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            placeholder="Search"
            className="w-full bg-transparent text-[16px] text-foreground outline-none placeholder:text-muted-foreground/50"
          />
          {query && (
            <button
              onClick={() => onQuery("")}
              className="text-muted-foreground/50 active:text-foreground"
            >
              <X className="size-4" />
            </button>
          )}
        </div>
      )}

      <div className="mx-0.5 h-6 w-px shrink-0 bg-border" />

      {fileActive ? (
        <NavBtn icon={Download} onClick={onFileDownload} title="Download" />
      ) : (
        <>
          <NavBtn icon={FolderPlus} onClick={onNewFolder} title="New folder" />
          <NavBtn icon={Upload} onClick={onUpload} title="Upload" />
          <NavBtn icon={Download} onClick={onDownload} title="Download" />
          <SegBtn
            icon={SidebarIcon}
            on={previewOpen}
            onClick={onTogglePreview}
            title="Quick Look"
          />
        </>
      )}
    </div>
  )
}

/** The toolbar breadcrumb trail — mobile twin. Same first · … · last-two
 *  collapse as desktop, scrollable; the crumb buttons are taller (36px) so each
 *  is a comfortable tap target. */
function ToolbarCrumbs({
  crumbs,
  onCrumb,
}: {
  crumbs: FinderNode[]
  onCrumb: (path: string) => void
}) {
  const collapsed = crumbs.length > 4
  const shown = collapsed ? [...crumbs.slice(0, 1), ...crumbs.slice(-2)] : crumbs
  return (
    <div className="no-scrollbar card-shadow flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto rounded-lg border border-border bg-card px-2 py-1">
      {shown.map((c, i) => {
        const isFirst = i === 0
        const showEllipsis = collapsed && i === 1
        return (
          <span key={c.path} className="flex shrink-0 items-center">
            {i > 0 && <ChevronRight className="size-3.5 text-muted-foreground/40" />}
            {showEllipsis && <ChevronRight className="size-3.5 text-muted-foreground/40" />}
            <button
              onClick={() => onCrumb(c.path)}
              className={cn(
                "rounded-sm px-2 py-1.5 text-[13px] transition-colors active:bg-muted/70",
                i === shown.length - 1 ? "font-semibold text-foreground" : "text-muted-foreground",
              )}
            >
              {isFirst ? (
                <span className="flex items-center gap-1">
                  <FileIcon kind="folder" size={16} />
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
  )
}

/** Mobile nav/action button — 36px touch target, no hover Tip (touch has no
 *  hover; the `title` carries the accessible name). */
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
      disabled={disabled}
      onClick={onClick}
      title={title}
      aria-label={title}
      className={cn(
        "flex size-9 shrink-0 items-center justify-center rounded-md transition-colors",
        disabled
          ? "cursor-not-allowed text-muted-foreground/30"
          : "text-muted-foreground/80 active:bg-muted/70 active:text-foreground",
      )}
    >
      <Icon className="size-4" />
    </button>
  )
}

/** Mobile toggle button — 36px, on-state accented. */
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
      onClick={onClick}
      title={title}
      aria-label={title}
      className={cn(
        "flex size-9 shrink-0 items-center justify-center rounded-md transition-colors",
        on
          ? "bg-muted text-(--signal)"
          : "text-muted-foreground active:bg-muted/70 active:text-foreground",
      )}
    >
      <Icon className="size-4" />
    </button>
  )
}

// FinderSidebar lives in support/; re-exported for path parity with the desktop
// chrome so existing importers resolve it from `./FinderChrome`. On mobile the
// Finder body drops the sidebar rail entirely (no room), so this re-export is
// never rendered — it exists only to keep the mirror's export surface identical.
export { FinderSidebar } from "./support/FinderSidebar"

// ── Path bar (bottom) ─────────────────────────────────────────────
// Mobile twin — taller crumb row (36px) for tapping, scrolls horizontally.
export function FinderPathBar({
  crumbs,
  onCrumb,
}: {
  crumbs: FinderNode[]
  onCrumb: (path: string) => void
}) {
  return (
    <div className="no-scrollbar flex h-9 shrink-0 items-center gap-0.5 overflow-x-auto border-t border-border bg-surface/80 px-3 text-[12px]">
      {crumbs.map((c, i) => (
        <span key={c.path} className="flex shrink-0 items-center">
          {i > 0 && <ChevronRight className="size-3.5 text-muted-foreground/40" />}
          <button
            onClick={() => onCrumb(c.path)}
            className="flex items-center gap-1 rounded-sm px-2 py-1.5 text-muted-foreground transition-colors active:bg-muted/60 active:text-foreground"
          >
            <FileIcon kind="folder" size={14} />
            {c.name}
          </button>
        </span>
      ))}
    </div>
  )
}
