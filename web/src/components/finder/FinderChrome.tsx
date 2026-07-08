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
  MoreHorizontal,
  PanelBottom,
  Search,
  Sidebar as SidebarIcon,
  Upload,
  X,
} from "lucide-react"
import type { FinderKind, FinderNode, FinderViewMode } from "@/lib/types"
import { extOf } from "./support/kind"
import { FileIcon } from "./support/macIcons"
import { Tip } from "@/components/ui/tip"
import { cn } from "@/lib/utils"
import { clickable } from "@/lib/support/a11y"

// Per-view-mode tooltip copy — the segmented control's icons aren't obvious.
const VIEW_TIP: Record<FinderViewMode, { title: string; body: string }> = {
  grid: { title: "Icons", body: "A grid of file & folder icons." },
  list: { title: "List", body: "A compact, sortable detail list." },
  columns: { title: "Columns", body: "Browse the hierarchy column by column (Miller)." },
  gallery: { title: "Gallery", body: "A large preview with a filmstrip of the rest." },
}

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
        return (
          <div
            key={t.id}
            {...clickable(() => onSelect(t.id))}
            className={cn(
              "group flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-[12px] transition-colors",
              on
                ? "card-shadow bg-card text-foreground"
                : "text-muted-foreground hover:bg-muted/60",
            )}
          >
            <FileIcon kind={t.kind} ext={extOf(t.label)} size={15} className="shrink-0" />
            <span className="max-w-[130px] truncate">{t.label}</span>
            {tabs.length > 1 && (
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  onClose(t.id)
                }}
                className="flex size-4 items-center justify-center rounded-sm text-muted-foreground/50 opacity-0 transition-opacity group-hover:opacity-100 hover:text-foreground"
              >
                <X className="size-3" />
              </button>
            )}
          </div>
        )
      })}
      <Tip title="New tab" body="Open another folder in a separate tab." side="bottom">
        <button
          onClick={onNew}
          className="flex size-6 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted/60 hover:text-foreground"
        >
          +
        </button>
      </Tip>
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
    <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border bg-surface px-3">
      <div className="flex items-center gap-0.5">
        <NavBtn icon={ArrowLeft} disabled={!canBack} onClick={onBack} title="Back" />
        <NavBtn icon={ArrowRight} disabled={!canForward} onClick={onForward} title="Forward" />
      </div>

      <ToolbarCrumbs crumbs={crumbs} onCrumb={onCrumb} />

      {/* icon-size slider (grid only) */}
      {!fileActive && viewMode === "grid" && (
        <div className="card-shadow flex items-center gap-1.5 rounded-lg border border-border bg-card px-2 py-1">
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
            className="card-shadow absolute inset-y-0.5 left-0.5 w-7 rounded-md bg-card transition-transform duration-200"
            style={{ transform: `translateX(${idx * 28}px)` }}
          />
          {VIEW_ORDER.map((m) => {
            const Icon = VIEW_ICON[m]
            return (
              <Tip key={m} title={VIEW_TIP[m].title} body={VIEW_TIP[m].body} side="bottom">
                <button
                  onClick={() => onViewMode(m)}
                  className={cn(
                    "relative z-1 flex size-7 items-center justify-center rounded-md transition-colors",
                    viewMode === m
                      ? "text-(--signal)"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  <Icon className="size-4" />
                </button>
              </Tip>
            )
          })}
        </div>
      )}

      {/* search */}
      {!fileActive && (
        <div className="card-shadow flex h-8 w-[156px] items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 focus-within:border-(--signal)/60">
          <Search className="size-3.5 shrink-0 text-muted-foreground/60" />
          <input
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            placeholder="Search realm"
            className="w-full bg-transparent text-[12px] text-foreground outline-none placeholder:text-muted-foreground/50"
          />
          {query && (
            <button
              onClick={() => onQuery("")}
              className="text-muted-foreground/50 hover:text-foreground"
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>
      )}

      <div className="mx-0.5 h-5 w-px bg-border" />

      {fileActive ? (
        <NavBtn icon={Download} onClick={onFileDownload} title="Download" />
      ) : (
        <>
          <NavBtn icon={FolderPlus} onClick={onNewFolder} title="New folder" />
          <NavBtn icon={Upload} onClick={onUpload} title="Upload" />
          <NavBtn icon={Download} onClick={onDownload} title="Download" />
          <SegBtn icon={PanelBottom} on={pathBarOpen} onClick={onTogglePathBar} title="Path bar" />
          <SegBtn
            icon={SidebarIcon}
            on={previewOpen}
            onClick={onTogglePreview}
            title="Quick Look pane"
          />
        </>
      )}
    </div>
  )
}

/** The toolbar breadcrumb trail. A deep path collapses to first · … · last-two
 *  so it never overflows the bar; the leading crumb carries a folder glyph and
 *  the trailing crumb is emphasised as the current directory. */
function ToolbarCrumbs({
  crumbs,
  onCrumb,
}: {
  crumbs: FinderNode[]
  onCrumb: (path: string) => void
}) {
  // collapse a deep breadcrumb: first · … · last two
  const collapsed = crumbs.length > 4
  // `collapsed` is only true when crumbs.length > 4, so index 0 is present —
  // assert it (noUncheckedIndexedAccess widens a bare `crumbs[0]`).
  const shown = collapsed ? [...crumbs.slice(0, 1), ...crumbs.slice(-2)] : crumbs
  return (
    <div className="card-shadow flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto rounded-lg border border-border bg-card px-2 py-1">
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
                "rounded-sm px-1.5 py-0.5 text-[12px] transition-colors hover:bg-muted/70",
                i === shown.length - 1 ? "font-semibold text-foreground" : "text-muted-foreground",
              )}
            >
              {isFirst ? (
                <span className="flex items-center gap-1">
                  <FileIcon kind="folder" size={15} />
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
  const btn = (
    <button
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
  // A disabled control swallows pointer events, so its tooltip never opens —
  // skip the wrapper entirely in that state.
  if (disabled) return btn
  return (
    <Tip title={title} side="bottom">
      {btn}
    </Tip>
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
    <Tip title={title} side="bottom">
      <button
        onClick={onClick}
        className={cn(
          "flex size-8 items-center justify-center rounded-md transition-colors",
          on
            ? "bg-muted text-(--signal)"
            : "text-muted-foreground hover:bg-muted/70 hover:text-foreground",
        )}
      >
        <Icon className="size-4" />
      </button>
    </Tip>
  )
}

// FinderSidebar (+ its private Group/Place helpers) lives in support/ to keep
// this file under the line budget; re-exported so existing importers keep
// resolving it from `./FinderChrome`.
export { FinderSidebar } from "./support/FinderSidebar"

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
            className="flex items-center gap-1 rounded-sm px-1 py-0.5 text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
          >
            <FileIcon kind="folder" size={13} />
            {c.name}
          </button>
        </span>
      ))}
    </div>
  )
}
