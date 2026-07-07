import type { FinderNode } from "@/lib/types"
import type { FinderTab } from "../FinderChrome"

/** A Finder tab: a folder browse context (with back/forward history) or, when
 *  `fileNode` is set, a single-file tab showing one file instead of a folder. */
export interface Tab extends FinderTab {
  back: string[]
  fwd: string[]
  /** when set, this is a file tab showing one file instead of a folder */
  fileNode?: FinderNode | undefined
}

/** A folder the user has pinned to the sidebar (persisted in localStorage). */
export interface PinnedFolder {
  name: string
  path: string
}

/** Single-click settle window (ms). A click defers its layout-affecting side
 *  effects (open the Quick Look pane, or arm slow-click-to-rename) by this much
 *  so a *double*-click can cancel them first — the first click of a double no
 *  longer opens the preview pane and reflows the grid out from under the second
 *  click. Shorter than a typical OS double-click threshold so a deliberate
 *  single click still feels responsive. */
export const CLICK_SETTLE_MS = 250

/** Sentinel `path` for the not-yet-created "New Folder" placeholder row. A NUL
 *  byte can never appear in a real realm path, so this never collides with a
 *  live entry; the inline editor keys off it to route a commit to mkdir (create)
 *  instead of rename. */
export const NEW_FOLDER_SENTINEL = "\u{0}__cp_new_folder__"

const pinsKeyFor = (agentId: string) => `cp-finder-pins:${agentId}`

/** localStorage key holding an agent's pinned folders. */
export const pinsKey = pinsKeyFor

/** Load an agent's pinned folders from localStorage (best-effort). */
export function loadPins(agentId: string): PinnedFolder[] {
  try {
    const raw = localStorage.getItem(pinsKeyFor(agentId))
    return raw ? (JSON.parse(raw) as PinnedFolder[]) : []
  } catch {
    return []
  }
}

/** Build breadcrumbs from a path relative to the agent's folder. */
export function buildCrumbs(agentFolder: string, agentName: string, cwd: string): FinderNode[] {
  if (cwd === agentFolder)
    return [{ name: agentName, path: agentFolder, kind: "folder", modified: "" }]
  const rel = cwd.startsWith(agentFolder + "/") ? cwd.slice(agentFolder.length + 1) : ""
  const parts = rel.split("/").filter(Boolean)
  const crumbs: FinderNode[] = [
    { name: agentName, path: agentFolder, kind: "folder", modified: "" },
  ]
  let cur = agentFolder
  for (const part of parts) {
    cur += `/${part}`
    crumbs.push({ name: part, path: cur, kind: "folder", modified: "" })
  }
  return crumbs
}

/** Extract the last segment of a path as a human label. */
export function pathName(p: string): string {
  const parts = p.split("/")
  return parts.at(-1) || p
}

/**
 * Return element `i` of `arr` (negative indexes count from the end, like
 * {@link Array.at}), throwing if it is absent.
 *
 * Used at call sites where a nearby length guard already proves the element is
 * present (e.g. `t.back.length ? … req(t.back, -1) …`). Under
 * `noUncheckedIndexedAccess` a bare `arr[i]` widens to `T | undefined`; this
 * encodes the real invariant with a runtime check instead of a non-null
 * assertion (`!`, banned by the P3 lint) — so a genuinely violated invariant
 * fails loudly at its source rather than surfacing as a downstream `undefined`.
 */
export function req<T>(arr: readonly T[], i: number): T {
  const v = arr.at(i)
  if (v === undefined) throw new Error(`index ${i} out of bounds (length ${arr.length})`)
  return v
}
