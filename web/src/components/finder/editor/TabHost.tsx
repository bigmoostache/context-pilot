import { X, FileText, SplitSquareHorizontal } from "lucide-react"
import { useDroppable } from "@dnd-kit/core"
import { SortableContext, horizontalListSortingStrategy, useSortable } from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import type { FinderNode } from "@/lib/types"
import { FinderPreview } from "../preview/FinderPreview"
import { VsCodeFileIcon } from "../support/VsCodeFileIcon"
import { cn } from "@/lib/utils"
import { type EditorGroup, type GroupsState, tabDragId, groupDropId } from "./tabState"

/**
 * The editor half of the Finder: the open editor GROUPS laid side by side
 * (T630 P3). Was a single tab strip + body; is now one {@link GroupView} per
 * group in a flex row, each an independent split with its own tabs and active
 * file. The active group is ringed; clicking anywhere in a group focuses it, so
 * the next file opened from the explorer lands there.
 *
 * THE BODY IS THE EXISTING {@link FinderPreview}, unchanged. That is the seam
 * this whole rework was built around: how a file is REACHED (a tree, a tab, a
 * split) is orthogonal to how it is RENDERED. So the ~1500 lines under
 * `preview/` are still untouched, and a tab body is one `variant="full"`
 * preview.
 */
export function TabHost({ groups, agentId }: { groups: GroupsState; agentId: string }) {
  return (
    <div className="m-2 flex min-h-0 min-w-0 flex-1 gap-2 overflow-hidden">
      {groups.groups.map((group) => (
        <GroupView
          key={group.id}
          group={group}
          groups={groups}
          agentId={agentId}
          isActive={group.id === groups.activeGroupId}
          // The last remaining group cannot be split away into nothing, but any
          // group can always be split; the split button is unconditional.
          canSplit
        />
      ))}
    </div>
  )
}

/**
 * One editor group — its tab strip and the body beneath it.
 *
 * A drop target (`group:<id>`) so a tab dragged from another group and released
 * anywhere over this one lands here, and an explorer file dropped here opens
 * here. The whole group is click-to-focus: mousing into its body sets it active
 * so the explorer opens into the split the user is looking at.
 */
function GroupView({
  group,
  groups,
  agentId,
  isActive,
  canSplit,
}: {
  group: EditorGroup
  groups: GroupsState
  agentId: string
  isActive: boolean
  canSplit: boolean
}) {
  const active = group.tabs.find((t) => t.path === group.activePath) ?? null
  const { setNodeRef, isOver } = useDroppable({ id: groupDropId(group.id) })

  return (
    <div
      ref={setNodeRef}
      // Focus-on-pointer-down (not click) so the group is already active by the
      // time a drag out of the explorer resolves its target.
      onPointerDownCapture={() => groups.setActiveGroup(group.id)}
      className={cn(
        "flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-md transition-shadow",
        isOver && "ring-2 ring-(--interactive)/60 ring-inset",
        // Only ring the active group when more than one exists — a lone group
        // needs no "which pane has focus" cue.
        !isOver &&
          isActive &&
          groups.groups.length > 1 &&
          "ring-1 ring-(--interactive)/30 ring-inset",
      )}
    >
      {group.tabs.length > 0 && <TabStrip group={group} groups={groups} canSplit={canSplit} />}
      {active ? (
        <FinderPreview
          // KEYED BY PATH so switching tabs REMOUNTS the preview. Without it,
          // React reuses the instance and every piece of per-file state inside
          // — scroll offset, the sheet's selected cell, a half-loaded image —
          // would leak from the tab left behind into the one arrived at.
          key={active.path}
          node={toNode(active)}
          agentId={agentId}
          variant="full"
          onClose={() => groups.close(group.id, active.path)}
        />
      ) : (
        <EmptyEditor />
      )}
    </div>
  )
}

/**
 * A tab back into the shape the preview wants.
 *
 * The strip stores only what a TAB needs (path, name, kind) rather than the
 * whole `FinderNode` it came from: a node carries a size and an mtime that go
 * stale the moment the agent writes the file, and a stale mtime pinned in tab
 * state would be shown as fact. The preview re-reads what it needs from the
 * realm anyway.
 */
function toNode(tab: { path: string; name: string; kind: FinderNode["kind"] }): FinderNode {
  return { name: tab.name, path: tab.path, kind: tab.kind, modified: "" }
}

/**
 * One group's horizontal tab strip, with the split button pinned to its right.
 *
 * `overflow-x-auto` and never wrapping: a strip that wraps to a second row
 * changes the height of the content area as tabs open, which moves the file
 * under the pointer. VS Code scrolls for the same reason.
 *
 * The DndContext that powers reorder + cross-group moves lives UP in the
 * {@link FinderShell}, spanning the explorer and every group; this strip only
 * declares the sortable list of ITS tabs. Sortable ids are namespaced by group
 * ({@link tabDragId}) so the same file open in two splits keeps two identities.
 */
function TabStrip({
  group,
  groups,
  canSplit,
}: {
  group: EditorGroup
  groups: GroupsState
  canSplit: boolean
}) {
  return (
    <div className="flex h-9 shrink-0 items-stretch bg-surface">
      <SortableContext
        items={group.tabs.map((t) => tabDragId(group.id, t.path))}
        strategy={horizontalListSortingStrategy}
      >
        <div className="flex min-w-0 flex-1 items-stretch overflow-x-auto overflow-y-hidden">
          {group.tabs.map((tab) => (
            <Tab
              key={tab.path}
              id={tabDragId(group.id, tab.path)}
              tab={tab}
              active={tab.path === group.activePath}
              onActivate={() => groups.activate(group.id, tab.path)}
              onClose={() => groups.close(group.id, tab.path)}
            />
          ))}
        </div>
      </SortableContext>
      {canSplit && (
        <button
          type="button"
          aria-label="Split editor right"
          title="Split editor right"
          onClick={() => {
            groups.setActiveGroup(group.id)
            groups.splitActive()
          }}
          className="flex w-8 shrink-0 items-center justify-center text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
        >
          <SplitSquareHorizontal className="size-3.5" />
        </button>
      )}
    </div>
  )
}

/**
 * One tab.
 *
 * A `<div>` with tab semantics rather than a `<button>`, because it CONTAINS
 * the close button — a button inside a button is invalid HTML and the browser
 * reparents it, which breaks the close click in a way that only shows up at
 * runtime. Its drag id ({@link tabDragId}) is namespaced by group so a
 * cross-group move can tell which split it came from.
 */
function Tab({
  id,
  tab,
  active,
  onActivate,
  onClose,
}: {
  id: string
  tab: OpenTabView
  active: boolean
  onActivate: () => void
  onClose: () => void
}) {
  // The whole tab is the drag handle. `useSortable` keyed by the namespaced id;
  // `isDragging` dims the lifted tab so the gap it leaves reads as the target.
  const { setNodeRef, attributes, listeners, transform, transition, isDragging } = useSortable({
    id,
  })

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      {...attributes}
      {...listeners}
      // After the dnd-kit spread so they WIN: `useSortable`'s attributes carry a
      // generic `role="button"` + `tabIndex`, but this element is a tab in a
      // tablist, so its own role/selected-state must override them.
      role="tab"
      aria-selected={active}
      tabIndex={0}
      onClick={onActivate}
      onKeyDown={(e) => {
        if (e.key !== "Enter" && e.key !== " ") return
        e.preventDefault()
        onActivate()
      }}
      onAuxClick={(e) => {
        // Middle click closes — the muscle memory every tabbed editor and
        // every browser shares. `onAuxClick` rather than `onMouseDown`, so a
        // middle-drag scroll does not destroy a tab.
        if (e.button !== 1) return
        e.preventDefault()
        onClose()
      }}
      className={cn(
        "group flex max-w-[220px] min-w-[120px] shrink-0 cursor-default items-center gap-1.5 px-3 text-[12.5px] transition-colors outline-none",
        isDragging && "z-10 opacity-60",
        active
          ? "bg-background text-foreground"
          : "bg-surface text-muted-foreground hover:text-foreground/90",
      )}
    >
      <VsCodeFileIcon name={tab.name} isFolder={tab.kind === "folder"} size={14} />
      {/* Italic while transient. It is the ONLY signal that this tab is about
          to be replaced by the next single click, and VS Code uses exactly
          this — inventing a different one would teach a wrong reflex. */}
      <span className={cn("min-w-0 flex-1 truncate", tab.preview && "italic")}>{tab.name}</span>
      <button
        type="button"
        aria-label={`Close ${tab.name}`}
        // Stop the sortable listeners from claiming a drag that starts on the
        // close button — otherwise a press-drag on the ✕ would lift the tab
        // instead of arming the close.
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          // The tab behind is clickable; without this, closing would also
          // activate the tab on its way out.
          e.stopPropagation()
          onClose()
        }}
        className={cn(
          "flex size-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground/70 transition-opacity hover:bg-muted hover:text-foreground",
          // Hidden until hover on an inactive tab, always visible on the
          // active one — otherwise closing the file you are looking at
          // requires finding an invisible control first.
          active ? "opacity-100" : "opacity-0 group-hover:opacity-100",
        )}
      >
        <X className="size-3" />
      </button>
    </div>
  )
}

/** The shape {@link Tab} reads off an open tab — a structural subset of the
 *  editor's `OpenTab` (path/name/kind/preview), kept local so this file does
 *  not re-import the whole state type just for a leaf render. */
interface OpenTabView {
  path: string
  name: string
  kind: FinderNode["kind"]
  preview: boolean
}

/** Shown when a group holds no file — VS Code's blank editor, with the one hint
 *  that actually resolves it. */
function EmptyEditor() {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 text-center">
      <span className="flex size-12 items-center justify-center rounded-2xl bg-muted text-muted-foreground/60">
        <FileText className="size-6" />
      </span>
      <p className="max-w-[320px] text-[13px] text-muted-foreground">
        Select a file in the explorer to open it here. Double-click to keep the tab.
      </p>
    </div>
  )
}
