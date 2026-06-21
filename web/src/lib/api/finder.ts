// ── Finder filesystem REST endpoints ────────────────────────────────
//
// Every read/mutation the Finder performs against an agent's realm, plus the
// raw inline-serve URL and the conversation feed. Split out of api/index for
// the file-size limit; re-exported there so `@/lib/api` stays the single
// import surface.

import type { FinderNode } from "../types"
import { BASE, request } from "./client"

export function fetchFs(agentId: string, path = ""): Promise<FinderNode[]> {
  const q = path ? `?path=${encodeURIComponent(path)}` : ""
  return request(`/api/agent/${agentId}/fs${q}`)
}

/** The agent's tree descriptions, as a flat `{ realmRelativePath: description }`
 *  map (`GET /api/agent/{id}/fs/descriptions`). Powers the Finder's per-node
 *  info badge: a node shows the ⓘ affordance exactly when its `path` is a key
 *  here. An agent with no descriptions yields `{}` (never an error). */
export function fetchDescriptions(agentId: string): Promise<Record<string, string>> {
  return request(`/api/agent/${agentId}/fs/descriptions`)
}

/** A file content preview from `GET /api/agent/{id}/fs/preview?path=`.
 *
 * The backend returns the first 256 KiB of a text file (`truncated` flags the
 * cap) and rejects binary content with a 415 — so a thrown `fetchFsPreview`
 * means "no text preview for this file", which the Finder renders as the plain
 * "No preview available" state. */
export interface FsPreview {
  content: string
  size: number
  truncated: boolean
}

/** Fetch a file's text content for the Finder Quick Look pane. Throws on a
 *  binary file (415) or read fault — callers fall back to the no-preview
 *  state. */
export function fetchFsPreview(agentId: string, path: string): Promise<FsPreview> {
  return request(`/api/agent/${agentId}/fs/preview?path=${encodeURIComponent(path)}`)
}

/** A spreadsheet parsed to tables by `GET /api/agent/{id}/fs/sheet?path=`.
 *
 * Every `csv`/`tsv`/`xlsx`/`xls`/`xlsb`/`ods` file collapses to the same shape:
 * a list of named sheets, each a grid of stringified cells (numbers/dates are
 * stringified server-side for display). `truncated` flags that a row/column/
 * sheet cap clipped the data so the UI can show a "preview clipped" note. A
 * non-spreadsheet file throws (415), which the Finder renders as the
 * no-preview state. */
export interface SheetData {
  sheets: { name: string; rows: string[][] }[]
  truncated: boolean
}

/** Fetch a spreadsheet's contents as tables for the Finder Quick Look pane.
 *  Throws on a non-spreadsheet / unparseable file (415) — callers fall back to
 *  the no-preview state. */
export function fetchSheet(agentId: string, path: string): Promise<SheetData> {
  return request(`/api/agent/${agentId}/fs/sheet?path=${encodeURIComponent(path)}`)
}

/** Result of a file write (`POST /fs/write`). */
export interface WriteResult {
  written: number
  path: string
}

/** Overwrite an existing realm file's contents (the Finder's in-place editor —
 *  e.g. saving the WYSIWYG markdown editor back to its `.md`). `path` is the
 *  file's realm-relative path; `content` is the new full text, sent as the raw
 *  request body. The backend confines the path and requires an existing regular
 *  file (escaping/absent → 403, directory → 400), so a save can only ever
 *  overwrite the file being edited. Throws on any non-2xx. */
export function writeFile(agentId: string, path: string, content: string): Promise<WriteResult> {
  return request(`/api/agent/${agentId}/fs/write?path=${encodeURIComponent(path)}`, {
    method: "POST",
    body: content,
  })
}

/** Result of a single-file upload (`POST /fs/upload`). */
export interface UploadResult {
  written: number
  path: string
}

/** Upload one file into a directory of the agent's realm. The body is the
 *  file's raw bytes; `dir` is the realm-relative destination directory (""
 *  = root). The Finder calls this once per selected file. Throws on a
 *  rejected name / confinement violation / write fault. */
export function uploadFile(agentId: string, dir: string, file: File): Promise<UploadResult> {
  const q = `path=${encodeURIComponent(dir)}&name=${encodeURIComponent(file.name)}`
  return request(`/api/agent/${agentId}/fs/upload?${q}`, {
    method: "POST",
    body: file,
  })
}

/** Result of a dedup-aware upload (`POST /fs/upload-unique`). */
export interface UploadUniqueResult {
  /** realm-relative path of the stored file (possibly suffixed on collision) */
  path: string
  /** final stored filename — `name (1).ext` etc. when the basename collided */
  name: string
  /** byte count written */
  size: number
}

/** Upload one file into a realm directory, **auto-creating** the directory and
 *  **never overwriting**: a name collision yields a ` (1)`, ` (2)`… suffix.
 *  Powers the threads chat composer's attachment flow (files land in the
 *  realm's `.uploads/`). The backend returns the stored path/name/size — all
 *  the composer needs to compose the `file-upload` message block. Throws on a
 *  rejected name / confinement violation / write fault. */
export function uploadUnique(
  agentId: string,
  dir: string,
  file: File,
): Promise<UploadUniqueResult> {
  const q = `path=${encodeURIComponent(dir)}&name=${encodeURIComponent(file.name)}`
  return request(`/api/agent/${agentId}/fs/upload-unique?${q}`, {
    method: "POST",
    body: file,
  })
}

/** Result of a folder creation (`POST /fs/mkdir`). */
export interface MkdirResult {
  created: string
}

/** Create a new folder `name` inside a realm directory (`dir`, "" = realm
 *  root). Powers the Finder's "New Folder" action (toolbar + empty-space
 *  context menu). The backend confines the parent dir, rejects a non-bare name
 *  or an already-existing entry (409), and returns the new folder's
 *  realm-relative path. */
export function createFolder(agentId: string, dir: string, name: string): Promise<MkdirResult> {
  const q = `path=${encodeURIComponent(dir)}&name=${encodeURIComponent(name)}`
  return request(`/api/agent/${agentId}/fs/mkdir?${q}`, { method: "POST" })
}

/** Result of a rename (`POST /fs/rename`). */
export interface RenameResult {
  /** the entry's new realm-relative path */
  renamed: string
}

/** Rename a single file or folder, keeping it in the same parent directory.
 *  `path` is the entry's realm-relative path, `newName` the bare new name (no
 *  path separators). Powers the Finder's inline rename. The backend confines
 *  the source, bare-name-guards `newName`, treats an unchanged name as a no-op,
 *  and refuses to clobber a different existing entry (409). Returns the new
 *  realm-relative path. */
export function renameItem(
  agentId: string,
  path: string,
  newName: string,
): Promise<RenameResult> {
  const q = `path=${encodeURIComponent(path)}&name=${encodeURIComponent(newName)}`
  return request(`/api/agent/${agentId}/fs/rename?${q}`, { method: "POST" })
}

/** Result of a move (`POST /fs/move`). */
export interface MoveResult {
  moved: number
  skipped: number
}

/** Move one or more realm-relative entries into a destination directory
 *  (`dest`, "" = realm root). Powers the Finder's internal drag-and-drop. The
 *  backend confines both sides, refuses to clobber an existing entry, and
 *  refuses to move a folder into its own descendant. */
export function moveItems(
  agentId: string,
  items: string[],
  dest: string,
): Promise<MoveResult> {
  return request(`/api/agent/${agentId}/fs/move`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ items, dest }),
  })
}

/** Result of a trash (`POST /fs/trash`). */
export interface TrashResult {
  trashed: number
  skipped: number
}

/** Move one or more realm-relative entries to the realm's hidden trash folder
 *  (the Finder's right-click "Move to Trash"). Reversible — the backend moves
 *  each entry into a hidden `.cp-trash/` dir at the realm root rather than
 *  destroying it (collisions get a timestamp suffix). The backend confines
 *  every source and skips anything already in the trash. */
export function trashItems(agentId: string, items: string[]): Promise<TrashResult> {
  return request(`/api/agent/${agentId}/fs/trash`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ items }),
  })
}

/** Build the URL that serves a realm file's raw bytes **inline** (no download
 *  prompt) — `GET /api/agent/{id}/fs/raw?path=`. Point an `<img src>` or
 *  `<object data>` at it to render the file in place (image & PDF Finder
 *  previews). The backend infers the `Content-Type` and caps the file at
 *  10 MiB; an oversized or unreadable file yields a non-2xx the tag renders as
 *  a broken element, which the preview components surface as a fallback. */
export function rawUrl(agentId: string, path: string): string {
  return `${BASE}/api/agent/${agentId}/fs/raw?path=${encodeURIComponent(path)}`
}

/** Trigger a browser download for a file in the agent's realm. */
export async function downloadFile(agentId: string, path: string): Promise<void> {
  const res = await fetch(
    `${BASE}/api/agent/${agentId}/fs/download?path=${encodeURIComponent(path)}`,
  )
  if (!res.ok) {
    const body = await res.text().catch(() => res.statusText)
    throw new Error(`${res.status}: ${body}`)
  }
  const blob = await res.blob()
  const filename =
    res.headers
      .get("Content-Disposition")
      ?.match(/filename="?([^"]+)"?/)?.[1] ?? path.split("/").pop() ?? "download"
  const url = URL.createObjectURL(blob)
  const a = document.createElement("a")
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

/** One raw conversation message from `/api/agent/{id}/conversation`.
 *
 * `id` is the agent's stable `Message::id` — the SAME id the durable
 * `MessageCreated` oplog entry and the ephemeral stream `Token` frame's
 * `message_id` carry — so a live token buffer can be correlated with its
 * durable message. `uid` is the on-disk file id (distinct, not used for
 * stream correlation). */
export interface ConversationMsg {
  id: string
  uid: string
  role: string
  content: string
  timestamp_ms: number
  /** "text" | "tool_call" | "tool_result" (others tolerated). */
  message_type?: string
  tool_uses?: Array<{ id?: string; name?: string; input?: Record<string, unknown> }>
  tool_results?: Array<{ tool_name?: string; content?: string; is_error?: boolean }>
}

export function fetchConversation(agentId: string): Promise<ConversationMsg[]> {
  return request(`/api/agent/${agentId}/conversation`)
}
