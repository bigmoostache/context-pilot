// ── Finder filesystem REST endpoints (SDK) ──────────────────────────
//
// All endpoints except rawUrl (URL builder) and downloadFile (binary
// download with Content-Disposition) use the generated SDK.  Types are
// re-exported from generated/types.gen so existing imports keep working.

import type {
  FinderNode,
  FsPreview,
  OfficeSaveResult,
  OfficeVersion,
  UploadResult,
  UploadUniqueResult,
  WriteResult,
} from "./generated/types.gen"
import {
  getApiFleetByIdFs,
  getApiFleetByIdFsDescriptions,
  getApiFleetByIdFsOfficeVersion,
  getApiFleetByIdFsPreview,
  postApiFleetByIdFsWrite,
} from "./generated"
import { BASE, getToken, sdk } from "./client"
import { sendUpload } from "./client/upload"

// ── Type re-exports (preserve import surface) ────────────────────────

export type { FinderNode, FsPreview, OfficeSaveResult, OfficeVersion } from "./generated/types.gen"
export type { WriteResult, UploadResult, UploadUniqueResult } from "./generated/types.gen"

/** An office document's bytes plus the version they were read at. */
export interface OfficeDoc {
  bytes: Uint8Array
  version: string
}

// ── GET endpoints (SDK) ──────────────────────────────────────────────

export function fetchFs(fleetId: string, path = ""): Promise<FinderNode[]> {
  return sdk(getApiFleetByIdFs({ path: { id: fleetId }, query: { path } }))
}

export function fetchDescriptions(fleetId: string): Promise<Record<string, string>> {
  return sdk(getApiFleetByIdFsDescriptions({ path: { id: fleetId } }))
}

export function fetchFsPreview(fleetId: string, path: string): Promise<FsPreview> {
  return sdk(getApiFleetByIdFsPreview({ path: { id: fleetId }, query: { path } }))
}

/** Current version of an office document (polled to pick up agent edits). */
export function fetchOfficeVersion(fleetId: string, path: string): Promise<OfficeVersion> {
  return sdk(getApiFleetByIdFsOfficeVersion({ path: { id: fleetId }, query: { path } }))
}

// ── POST endpoints (SDK) ─────────────────────────────────────────────

export function writeFile(fleetId: string, path: string, content: string): Promise<WriteResult> {
  return sdk(postApiFleetByIdFsWrite({ path: { id: fleetId }, query: { path }, body: content }))
}

/**
 * Save a whole office document back to disk through the upload channel.
 * `base` = the version it was loaded at; the backend answers 409 when an
 * agent rewrote the file since.
 */
export function saveOffice(
  fleetId: string,
  path: string,
  bytes: Uint8Array,
  base: string,
): Promise<OfficeSaveResult> {
  const q = `path=${encodeURIComponent(path)}&base=${encodeURIComponent(base)}`
  const name = path.split("/").pop() ?? path
  const blob = new Blob([bytes as BlobPart], { type: "application/octet-stream" })
  return sendUpload<OfficeSaveResult>(`/api/fleet/${fleetId}/fs/office?${q}`, blob, name)
}

/** Upload one file into `dir` (must exist). Goes through the single upload
 *  channel (progress toast, no size cap). */
export function uploadFile(fleetId: string, dir: string, file: File): Promise<UploadResult> {
  const q = `path=${encodeURIComponent(dir)}&name=${encodeURIComponent(file.name)}`
  return sendUpload<UploadResult>(`/api/fleet/${fleetId}/fs/upload?${q}`, file, file.name)
}

/** Upload into `dir` (auto-created), never overwriting. Same channel. */
export function uploadUnique(
  fleetId: string,
  dir: string,
  file: File,
): Promise<UploadUniqueResult> {
  const q = `path=${encodeURIComponent(dir)}&name=${encodeURIComponent(file.name)}`
  return sendUpload<UploadUniqueResult>(
    `/api/fleet/${fleetId}/fs/upload-unique?${q}`,
    file,
    file.name,
  )
}

// ── Manual endpoints (irreducible — URL builder + binary download) ───

/** Build the URL that serves a realm file's raw bytes inline (no download
 *  prompt). `fs/raw` is NOT a public route, so an `<img src>` / `<object data>`
 *  pointed straight at this URL can't carry the `Authorization` header and 401s
 *  once auth is on (C2). Kept only for the rare same-origin/no-auth caller. */
export function rawUrl(fleetId: string, path: string): string {
  return `${BASE}/api/fleet/${fleetId}/fs/raw?path=${encodeURIComponent(path)}`
}

/** Fetch an office document's bytes and the version they were read at. */
export async function fetchOffice(fleetId: string, path: string): Promise<OfficeDoc> {
  const headers: Record<string, string> = {}
  const token = getToken()
  if (token) headers["Authorization"] = `Bearer ${token}`
  const res = await fetch(
    // ok:manual — binary body + X-Office-Version header, irreducible
    `${BASE}/api/fleet/${fleetId}/fs/office?path=${encodeURIComponent(path)}`,
    { headers, cache: "no-store" },
  )
  if (!res.ok) {
    const body = await res.text().catch(() => res.statusText)
    throw new Error(`${res.status}: ${body}`)
  }
  const bytes = new Uint8Array(await res.arrayBuffer())
  return { bytes, version: res.headers.get("X-Office-Version") ?? "" }
}

/** Trigger a browser download for a file in the agent's realm. */
export async function downloadFile(fleetId: string, path: string): Promise<void> {
  const headers: Record<string, string> = {}
  const token = getToken()
  if (token) headers["Authorization"] = `Bearer ${token}`

  const res = await fetch(
    // ok:manual — binary blob download, irreducible
    `${BASE}/api/fleet/${fleetId}/fs/download?path=${encodeURIComponent(path)}`,
    { headers },
  )
  if (!res.ok) {
    const body = await res.text().catch(() => res.statusText)
    throw new Error(`${res.status}: ${body}`)
  }
  const blob = await res.blob()
  const filename =
    res.headers.get("Content-Disposition")?.match(/filename="?([^"]+)"?/)?.[1] ??
    path.split("/").pop() ??
    "download"
  saveBlob(blob, filename)
}

/** Hand a client-side blob to the browser as a file download named `filename`. */
export function saveBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob)
  const a = document.createElement("a")
  a.href = url
  a.download = filename
  a.click()
  // Defer the revoke: revoking synchronously after click() can race the
  // browser's own read of the blob and abort the download in some browsers.
  // 10s is comfortably past the navigation the click kicks off.
  window.setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

/**
 * Download a browser-rendered SVG string as a standalone file: the root's
 * responsive `width="100%"` becomes the viewBox's fixed size (standalone
 * viewers otherwise pick an arbitrary one) and `background` is baked into the
 * root style, replacing mermaid's `max-width` cap.
 */
export function saveSvg(svg: string, background: string, filename: string): void {
  const vb = /viewBox="([^"]+)"/.exec(svg)?.[1]?.split(/\s+/) ?? []
  const out = svg
    .replace(/\swidth="100%"/, () => ` width="${vb[2] ?? 0}" height="${vb[3] ?? 0}"`)
    .replace(/max-width:\s*[\d.]+px;?/, () => `background-color: ${background};`)
  saveBlob(new Blob([out], { type: "image/svg+xml" }), filename)
}
