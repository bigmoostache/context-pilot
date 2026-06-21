import type { DragEvent as ReactDragEvent } from "react"
import { useState } from "react"
import { ChevronRight } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { fmtBytes, sortNodes } from "@/lib/support/finderFs"
import { useFs } from "@/lib/live"
import { extOf, kindMeta } from "../support/kind"
import { FileIcon } from "../support/macIcons"
import { InfoBadge } from "../support/InfoBadge"
import { cn } from "@/lib/utils"
import {
  folderDropProps,
  isMoveDrag,
  mods,
  readMovePayload,
  relOf,
  RenameInput,
  startItemDrag,
  TagDots,
  type ViewHandlers,
} from "./shared"

/**
 * Miller-columns browser over LIVE data. One column per ancestor in the path
 * chain (root → cwd): each column lists that folder's children, with the child
 * that leads to the next column highlighted, so the whole traversed hierarchy
 * is visible at once (the point of column view) — not just the current folder.
 *
 * Every ancestor column fetches its own children via `useFs`; the deepest
 * column reuses the already-fetched + filtered + sorted `currentNodes`. A
 * trailing pane previews the selected file. Clicking a folder in any column
 * navigates into it (truncating the chain past that point).
 */
export function ColumnsView({
  agentId,
  agentFolder,
  chain,
  currentNodes,
  previewNode,
  onNavigate,
  ...h
}: ViewHandlers & {
  agentId: string
  agentFolder: string
  /** absolute paths from realm root down to the current working directory */
  chain: string[]
  /** the current dir's already-filtered+sorted nodes (deepest column) */
  currentNodes: FinderNode[]
  previewNode: FinderNode | null
  onNavigate: (path: string) => void
}) {
  const showPreviewPane = previewNode && previewNode.kind !== "folder"
  return (
    <div className="flex h-full min-w-0 overflow-x-auto">
      {chain.map((path, i) => (
        <MillerColumn
          key={path}
          agentId={agentId}
          agentFolder={agentFolder}
          path={path}
          nextPath={chain[i + 1]}
          nodes={i === chain.length - 1 ? currentNodes : undefined}
          onNavigate={onNavigate}
          {...h}
        />
      ))}

      {showPreviewPane && previewNode && (
        <div className="flex w-[230px] shrink-0 flex-col items-center gap-3 px-5 py-7 text-center">
          <FileIcon kind={previewNode.kind} ext={extOf(previewNode.name)} size={84} />
          <span className="text-[13px] font-semibold text-foreground/90">{previewNode.name}</span>
          <TagDots tags={previewNode.tags} />
          <dl className="mt-1 flex w-full flex-col gap-1 text-[11px]">
            <PaneRow k="Kind" v={kindMeta[previewNode.kind].label} />
            <PaneRow k="Size" v={fmtBytes(previewNode.size)} />
            <PaneRow k="Modified" v={previewNode.modified} />
          </dl>
        </div>
      )}
    </div>
  )
}

/**
 * One Miller column = the listing of a single folder in the path chain. Ancestor
 * columns fetch their own children live; the deepest column receives the current
 * dir's nodes directly (so search/sort already applied). The row whose path is
 * `nextPath` (the traversed child) is highlighted as "on trail".
 */
function MillerColumn({
  agentId,
  agentFolder,
  path,
  nextPath,
  nodes: provided,
  onNavigate,
  ...h
}: ViewHandlers & {
  agentId: string
  agentFolder: string
  path: string
  nextPath?: string
  /** present for the deepest column (current dir) — skips the fetch */
  nodes?: FinderNode[]
  onNavigate: (path: string) => void
}) {
  // Hook is always called (rules of hooks); its data is ignored when `provided`.
  const { data } = useFs(agentId, relOf(agentFolder, path))
  const nodes = sortNodes(provided ?? data ?? [], "name", true)
  const [dragOver, setDragOver] = useState<string | null>(null)
  // Whether a move-drag is hovering the column's BODY (its background / a file
  // row), as opposed to a folder row inside it (which owns its own highlight via
  // `dragOver`). A body drop moves the dragged items into THIS column's folder.
  const [bodyOver, setBodyOver] = useState(false)

  // The column's own folder, as a destination for a body drop. Synthetic node —
  // `onMove` only reads `.path`/`.name`.
  const columnFolder: FinderNode = {
    name: path.split("/").pop() || path,
    path,
    kind: "folder",
    modified: "",
  }

  // Drop-target handlers for the column BODY: a move-drag that lands outside any
  // folder row (the folder rows `stopPropagation` their own drop, so those never
  // reach here) moves the dragged items into this column's folder. Highlight is
  // suppressed while a folder row is hovered (`dragOver !== null`) so the two
  // never light up at once. A no-op when `onMove` is absent or the drag isn't an
  // internal move.
  const columnDropProps = h.onMove
    ? {
        onDragOver: (e: ReactDragEvent) => {
          if (!isMoveDrag(e)) return
          const dragged = readMovePayload(e)
          if (dragged?.includes(path)) return // can't drop the folder into itself
          e.preventDefault()
          e.dataTransfer.dropEffect = "move"
          if (!bodyOver) setBodyOver(true)
        },
        onDragLeave: (e: ReactDragEvent) => {
          // Only clear when the cursor actually leaves the whole column, not when
          // it moves onto a child row.
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
            setBodyOver(false)
          }
        },
        onDrop: (e: ReactDragEvent) => {
          if (!isMoveDrag(e)) return
          e.preventDefault()
          setBodyOver(false)
          const dragged = readMovePayload(e)
          if (dragged && !dragged.includes(path)) h.onMove?.(dragged, columnFolder)
        },
      }
    : {}

  const showBodyOver = bodyOver && dragOver === null
  return (
    <div
      {...columnDropProps}
      className={cn(
        "flex w-[218px] shrink-0 flex-col overflow-y-auto border-r border-border py-1",
        showBodyOver && "bg-[var(--signal)]/8 ring-1 ring-inset ring-[var(--signal)]/40",
      )}
    >
      {nodes.map((n) => {
        // The traversed child of THIS column (the folder opened to spawn the
        // next column) is highlighted so the whole navigation path reads at a
        // glance. `nextPath` comes from the crumb chain (absolute, agent-folder
        // rooted) while listing nodes carry realm-relative paths, so normalise
        // `nextPath` to the same relative form before comparing — otherwise the
        // two never match and no ancestor ever lights up (T287).
        const onTrail = nextPath != null && n.path === relOf(agentFolder, nextPath)
        const sel = h.selected.has(n.path)
        const dropOver = dragOver === n.path
        return (
          <button
            key={n.path}
            draggable
            onDragStart={(e) => startItemDrag(e, n, h.selected)}
            {...folderDropProps(n, dropOver, setDragOver, h.onMove)}
            onClick={(e) => {
              h.onClick(n, mods(e))
              if (n.kind === "folder") onNavigate(n.path)
            }}
            onDoubleClick={() => h.onOpen(n)}
            onContextMenu={(e) => h.onContext(e, n)}
            className={cn(
              // `select-none` is load-bearing here: unlike grid/list (whose
              // <main> carries select-none via the marquee), the columns view
              // has none, so a native press-drag on a row's text would start a
              // TEXT selection instead of an element drag — the drag never
              // fires and the move silently fails (T287). Suppressing text
              // selection lets `draggable` initiate the element drag reliably.
              "mx-1 flex select-none items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors",
              // A folder that is OPEN in the path we're traversing (its children
              // fill the next column) gets the SAME prominent signal background
              // as a selected row — so the whole opened chain reads as a
              // connected trail down the columns at a glance (T287). Background
              // only: the user explicitly asked to drop the left accent bar.
              onTrail || sel
                ? "bg-[var(--signal)]/20 font-medium text-foreground"
                : "text-foreground/80 hover:bg-muted/45",
              dropOver && "bg-[var(--signal)]/20 ring-1 ring-inset ring-[var(--signal)]/70",
            )}
          >
            <FileIcon kind={n.kind} ext={extOf(n.name)} size={17} className="shrink-0" />
            {h.renamingPath === n.path && h.onRenameCommit && h.onRenameCancel ? (
              <RenameInput
                node={n}
                onCommit={(name) => h.onRenameCommit?.(n, name)}
                onCancel={() => h.onRenameCancel?.()}
              />
            ) : (
              <span className="min-w-0 flex-1 truncate font-medium">{n.name}</span>
            )}
            <TagDots tags={n.tags} />
            {h.descriptions?.[n.path] && <InfoBadge description={h.descriptions[n.path]} />}
            {n.kind === "folder" && (
              <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/50" />
            )}
          </button>
        )
      })}
    </div>
  )
}

function PaneRow({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex items-baseline justify-between gap-2">
      <dt className="text-muted-foreground">{k}</dt>
      <dd className="truncate text-foreground/80">{v}</dd>
    </div>
  )
}
