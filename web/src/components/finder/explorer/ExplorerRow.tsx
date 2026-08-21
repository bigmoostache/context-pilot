import type { FinderNode } from "@/lib/types"
import { ChevronRight } from "lucide-react"
import { FileIcon } from "../support/macIcons"
import { extOf } from "../support/kind"
import { InfoBadge } from "../support/InfoBadge"
import { cn } from "@/lib/utils"

/**
 * One line of the explorer tree.
 *
 * PURELY PRESENTATIONAL — it is handed a depth and a set of flags and knows
 * nothing about the filesystem, which is what lets {@link ExplorerTree} recurse
 * without this component recursing with it.
 *
 * THE INDENT GUIDES ARE NOT DECORATION. At depth four or five, a tree indented
 * with whitespace alone gives the eye nothing to track a column by, and reading
 * which folder a file belongs to becomes a matter of counting pixels. The
 * hairlines are what make a deep tree scannable, and they are the single most
 * recognisable thing about a VS Code explorer.
 */
export function ExplorerRow({
  node,
  depth,
  expanded,
  active,
  description,
  onActivate,
  onPin,
  onContext,
}: {
  node: FinderNode
  /** 0 at the realm root's children. Drives both the indent and the guides. */
  depth: number
  /** Folders only — drives the chevron's rotation. */
  expanded: boolean
  /** This file is the active tab, or this folder is the active file's parent. */
  active: boolean
  /** Tree description, when the agent has written one. Renders the ⓘ badge. */
  description: string | undefined
  /** Single click: toggle a folder, or open a file in a preview tab. */
  onActivate: () => void
  /** Double click on a FILE: pin the tab. Folders pass undefined. */
  onPin?: (() => void) | undefined
  onContext: (e: React.MouseEvent) => void
}) {
  const isFolder = node.kind === "folder"

  return (
    <button
      type="button"
      onClick={onActivate}
      onDoubleClick={onPin}
      onContextMenu={onContext}
      // `title` deliberately, not a `Tip`: a tooltip on every row of a
      // hundred-row tree is a hundred portalled popups and a wall of hover
      // chrome. The native one is quiet and only answers a truncated name.
      title={node.name}
      className={cn(
        "group relative flex h-[22px] w-full items-center gap-1 pr-2 text-left text-[13px] transition-colors",
        active ? "bg-(--interactive)/12 text-foreground" : "text-foreground/80 hover:bg-muted/60",
      )}
    >
      {/* One guide per ANCESTOR level, drawn as a left border on a fixed-width
          spacer. Absolute positioning was the alternative and is worse: these
          participate in the flex row, so the content after them lands at the
          correct indent by construction rather than by a matching padding
          value kept in sync by hand. */}
      {Array.from({ length: depth }, (_, i) => (
        <span
          key={i}
          aria-hidden
          className="h-full w-[9px] shrink-0 border-l border-(--border-strong)/55"
        />
      ))}

      {/* The chevron column exists on FILES TOO, as an empty spacer: without it
          a file's icon would sit 14px left of its sibling folders' icons and
          the tree would look ragged at every level. */}
      <span className="flex size-[14px] shrink-0 items-center justify-center text-muted-foreground/70">
        {isFolder && (
          <ChevronRight className={cn("size-3.5 transition-transform", expanded && "rotate-90")} />
        )}
      </span>

      <FileIcon kind={node.kind} ext={extOf(node.name)} size={15} />

      <span className="min-w-0 flex-1 truncate">{node.name}</span>

      {description !== undefined && <InfoBadge description={description} />}
    </button>
  )
}
