import { useCallback, useMemo, useState } from "react"

/**
 * Which folders are open in the explorer, and how a path is revealed.
 *
 * THE EXPANDED SET IS THE WHOLE MODEL. The tree deliberately holds no copy of
 * the filesystem: each open folder fetches its own children through `useFs`
 * (see {@link ExplorerNode}), so this state is the only thing the explorer
 * itself owns. That is what makes the tree cheap — a folder nobody opened is a
 * folder nobody fetched — and it is why collapsing cannot go stale: there is no
 * cached listing to invalidate, only a key removed from a set.
 *
 * Paths here are ABSOLUTE, matching `FinderNode.path`. The relative form the
 * API wants is derived at the fetch site by {@link relOf}; keeping absolute
 * paths as the identity means a row can be compared to the active tab without
 * either side re-deriving anything.
 */
export interface TreeState {
  /** Absolute paths of every open folder. */
  readonly expanded: ReadonlySet<string>
  /** Open a folder if closed, close it if open. */
  toggle: (path: string) => void
  /** Open a folder and every ancestor of it. Idempotent. */
  reveal: (path: string, agentFolder: string) => void
}

export function useTreeState(): TreeState {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set())

  const toggle = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      // `delete` reports whether it removed anything, so the closed case needs
      // no separate membership test.
      if (!next.delete(path)) next.add(path)
      return next
    })
  }, [])

  /**
   * Open every folder ON THE WAY to a path — not the path itself.
   *
   * Used by "Show in Finder": revealing `src/lib/api/index.ts` has to open
   * `src`, `src/lib` and `src/lib/api`, or the row simply is not rendered and
   * there is nothing to scroll to. The leaf is deliberately left alone: if it
   * is a file it has no children, and if it is a folder the user asked to see
   * it, not to see inside it.
   */
  const reveal = useCallback((path: string, agentFolder: string) => {
    const rel = relOf(agentFolder, path)
    if (rel === "") return

    setExpanded((prev) => {
      const next = new Set(prev)
      const segments = rel.split("/")
      // Every ancestor, rebuilt from the realm root down. Dropping the leaf
      // (the last segment) — its children are not what a reveal opens.
      const ancestors = segments.slice(0, -1)
      let walk = agentFolder
      for (const segment of ancestors) {
        walk += `/${segment}`
        next.add(walk)
      }
      return next
    })
  }, [])

  return useMemo(() => ({ expanded, toggle, reveal }), [expanded, toggle, reveal])
}

/**
 * An absolute realm path as the API wants it — relative to the agent's folder.
 *
 * `confined_path` on the backend REJECTS an absolute path outright, so this
 * conversion is not cosmetic. The realm root maps to `""`, which is what the
 * listing endpoint expects for "the top of the realm".
 *
 * A path that does not sit under the realm is returned untouched rather than
 * mangled: it cannot be made relative to a folder it is not inside, and
 * silently trimming it would produce a plausible-looking path pointing
 * somewhere else entirely.
 */
export function relOf(agentFolder: string, path: string): string {
  if (path === agentFolder) return ""
  return path.startsWith(`${agentFolder}/`) ? path.slice(agentFolder.length + 1) : path
}

/**
 * Folders first, then files, each group A-Z — the order every file explorer
 * uses and the only one in which a deep tree stays scannable.
 *
 * `localeCompare` with `numeric` so `item2` sorts before `item10`, which plain
 * lexicographic ordering gets backwards.
 */
export function sortTreeNodes<T extends { name: string; kind: string }>(nodes: readonly T[]): T[] {
  return [...nodes].toSorted((a, b) => {
    const aFolder = a.kind === "folder"
    const bFolder = b.kind === "folder"
    if (aFolder !== bFolder) return aFolder ? -1 : 1
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" })
  })
}
