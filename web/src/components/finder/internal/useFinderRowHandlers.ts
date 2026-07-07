import { useEffect, useMemo, useRef } from "react"
import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from "react"
import type { FinderNode } from "@/lib/types"
import { startItemDrag } from "../views/shared"

interface RowHandlerDeps {
  onRowClick: (n: FinderNode, m: { additive: boolean; range: boolean }) => void
  open: (n: FinderNode) => void
  openContext: (e: ReactMouseEvent, n: FinderNode) => void
  moveItemsInto: (paths: string[], dest: FinderNode) => void
  commitRename: (n: FinderNode, name: string) => void
  cancelRename: () => void
  selected: Set<string>
}

/**
 * M26: stable handler identities so the memoised Finder rows don't re-render on
 * every selection change. The selection/action handlers close over live state
 * and are intentionally recreated each render (fresh closures avoid stale
 * reads); a latest-ref snapshot lets the rows keep referentially-stable
 * callbacks while those callbacks still invoke the current closures. `selected`
 * rides along so `onDragStart` can carry the whole live selection on a multi-
 * drag without the Set's per-selection identity busting the row memo.
 */
export function useFinderRowHandlers(deps: RowHandlerDeps) {
  const liveHandlers = useRef(deps)
  useEffect(() => {
    liveHandlers.current = deps
  })
  return useMemo(
    () => ({
      onClick: (n: FinderNode, m: { additive: boolean; range: boolean }) => liveHandlers.current.onRowClick(n, m),
      onOpen: (n: FinderNode) => liveHandlers.current.open(n),
      onContext: (e: ReactMouseEvent, n: FinderNode) => liveHandlers.current.openContext(e, n),
      onMove: (paths: string[], dest: FinderNode) => liveHandlers.current.moveItemsInto(paths, dest),
      onRenameCommit: (n: FinderNode, name: string) => liveHandlers.current.commitRename(n, name),
      onRenameCancel: () => liveHandlers.current.cancelRename(),
      onDragStart: (e: ReactDragEvent, n: FinderNode) => startItemDrag(e, n, liveHandlers.current.selected),
    }),
    [],
  )
}
