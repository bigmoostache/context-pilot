import type { FinderNode, FinderSortKey } from "../types"

// ── Finder node-tree helpers ──────────────────────────────────────────
//
// Pure utilities over the live realm tree the Finder fetches from the backend
// (`useFs`). The former mock-realm builder (buildRealm + its sample payloads)
// was deleted once every Finder surface moved to live data (M63 demaquetting).

/** Flatten every starred node in the realm (for the Favorites sidebar). */
export function collectStarred(root: FinderNode): FinderNode[] {
  const out: FinderNode[] = []
  const walk = (n: FinderNode) => {
    if (n.starred && n.path !== root.path) out.push(n)
    const kids = n.children ?? []
    for (const c of kids) walk(c)
  }
  walk(root)
  return out
}

/** Human-readable byte size. */
export function fmtBytes(n?: number | null): string {
  if (n == null) return "—"
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`
}

/** Folders first, then by the chosen key. */
export function sortNodes(nodes: FinderNode[], key: FinderSortKey, asc: boolean): FinderNode[] {
  const dir = asc ? 1 : -1
  return nodes.toSorted((a, b) => {
    const ad = a.kind === "folder"
    const bd = b.kind === "folder"
    if (ad !== bd) return ad ? -1 : 1
    let cmp: number
    switch (key) {
      case "name": {
        cmp = a.name.localeCompare(b.name)
        break
      }
      case "size": {
        cmp = (a.size ?? 0) - (b.size ?? 0)
        break
      }
      case "kind": {
        cmp = a.kind.localeCompare(b.kind)
        break
      }
      default: {
        cmp = a.name.localeCompare(b.name)
      }
    }
    return cmp * dir
  })
}
