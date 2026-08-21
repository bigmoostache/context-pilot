import { X, FileText } from "lucide-react"
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core"
import { SortableContext, horizontalListSortingStrategy, useSortable } from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import type { FinderNode } from "@/lib/types"
import { FinderPreview } from "../preview/FinderPreview"
import { VsCodeFileIcon } from "../support/VsCodeFileIcon"
import { cn } from "@/lib/utils"
import type { OpenTab, TabsState } from "./tabState"

/**
 * The tab strip and the body beneath it — the content half of the Finder.
 *
 * THE BODY IS THE EXISTING {@link FinderPreview}, unchanged. That is the seam
 * this whole rework was built around: how a file is REACHED (a tree, a tab) is
 * orthogonal to how it is RENDERED (syntax highlighting, sheet grids, live
 * image/PDF fetches). So the ~1500 lines under `preview/` were carried across
 * untouched, and a tab body is one `variant="full"` preview.
 */
export function TabHost({ tabs, agentId }: { tabs: TabsState; agentId: string }) {
  const active = tabs.tabs.find((t) => t.path === tabs.activePath) ?? null

  return (
    <div className="m-2 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
      {tabs.tabs.length > 0 && <TabStrip tabs={tabs} />}
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
          onClose={() => tabs.close(active.path)}
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
function toNode(tab: OpenTab): FinderNode {
  return { name: tab.name, path: tab.path, kind: tab.kind, modified: "" }
}

/**
 * The horizontal tab strip.
 *
 * `overflow-x-auto` and never wrapping: a strip that wraps to a second row
 * changes the height of the content area as tabs open, which moves the file
 * under the pointer. VS Code scrolls for the same reason.
 */
function TabStrip({ tabs }: { tabs: TabsState }) {
  // A 4px activation distance so a plain click (activate / close / middle-close)
  // is NOT swallowed as a drag: the pointer must travel before a reorder starts,
  // which leaves every existing click affordance on the tab intact.
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }))

  const onDragEnd = (e: DragEndEvent) => {
    const { active, over } = e
    if (over && active.id !== over.id) tabs.reorder(String(active.id), String(over.id))
  }

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
      <SortableContext
        items={tabs.tabs.map((t) => t.path)}
        strategy={horizontalListSortingStrategy}
      >
        <div className="flex h-9 shrink-0 items-stretch overflow-x-auto overflow-y-hidden border-b border-(--border-strong)/70 bg-surface">
          {tabs.tabs.map((tab) => (
            <Tab
              key={tab.path}
              tab={tab}
              active={tab.path === tabs.activePath}
              onActivate={() => tabs.activate(tab.path)}
              onClose={() => tabs.close(tab.path)}
            />
          ))}
        </div>
      </SortableContext>
    </DndContext>
  )
}

/**
 * One tab.
 *
 * A `<div>` with {@link clickableTab} semantics rather than a `<button>`,
 * because it CONTAINS the close button — a button inside a button is invalid
 * HTML and the browser reparents it, which breaks the close click in a way that
 * only shows up at runtime.
 */
function Tab({
  tab,
  active,
  onActivate,
  onClose,
}: {
  tab: OpenTab
  active: boolean
  onActivate: () => void
  onClose: () => void
}) {
  // The whole tab is the drag handle. `useSortable` keyed by the tab's path (its
  // stable identity); `isDragging` dims the lifted tab so the gap it leaves reads
  // as the drop target.
  const { setNodeRef, attributes, listeners, transform, transition, isDragging } = useSortable({
    id: tab.path,
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
        "group flex max-w-[220px] min-w-[120px] shrink-0 cursor-default items-center gap-1.5 border-r border-(--border-strong)/70 px-3 text-[12.5px] transition-colors outline-none",
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

/** Shown when no file is open — VS Code's blank editor, with the one hint that
 *  actually resolves it. */
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
