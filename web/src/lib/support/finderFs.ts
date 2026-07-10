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

/** Human-readable "time ago" for a modified timestamp; leaves already-formatted
 *  strings untouched and falls back to an absolute date past a month. */
export function fmtModified(v: string | number | null | undefined): string {
  if (v == null || v === "") return "—"
  // Already human ("2d ago", "Yesterday", …) — leave the mock's format intact.
  if (typeof v === "string" && /^\d+$/.exec(v.trim()) === null) return v
  const ms = typeof v === "number" ? v : Number(v)
  if (!Number.isFinite(ms)) return "—"
  const diff = Date.now() - ms
  if (diff < 0) return "just now"
  const sec = Math.floor(diff / 1000)
  if (sec < 60) return "just now"
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m ago`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr}h ago`
  const day = Math.floor(hr / 24)
  if (day < 7) return `${day}d ago`
  const wk = Math.floor(day / 7)
  if (wk < 5) return `${wk}w ago`
  // Older than a month → an absolute date reads better than "9w ago".
  return new Date(ms).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  })
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
