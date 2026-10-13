// ── Finder filesystem REST endpoints (SDK) ──────────────────────────
//
// All endpoints except rawUrl (URL builder) and downloadFile (binary
// download with Content-Disposition) use the generated SDK.  Types are
// re-exported from generated/types.gen so existing imports keep working.

import type {
  FinderNode,
  FsPreview,
  SheetData,
  SheetSaveResult,
  UploadResult,
  UploadUniqueResult,
  WriteResult,
} from "./generated/types.gen"
import {
  getApiFleetByIdFs,
  getApiFleetByIdFsDescriptions,
  getApiFleetByIdFsPreview,
  getApiFleetByIdFsSheet,
  postApiFleetByIdFsSheetSave,
  postApiFleetByIdFsUpload,
  postApiFleetByIdFsUploadUnique,
  postApiFleetByIdFsWrite,
} from "./generated"
import { BASE, getToken, sdk } from "./client"

// ── Type re-exports (preserve import surface) ────────────────────────

export type { FinderNode, FsPreview, SheetData, SheetSaveResult } from "./generated/types.gen"
export type { WriteResult, UploadResult, UploadUniqueResult } from "./generated/types.gen"

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

export function fetchSheet(fleetId: string, path: string): Promise<SheetData> {
  return sdk(getApiFleetByIdFsSheet({ path: { id: fleetId }, query: { path } }))
}

// ── POST endpoints (SDK) ─────────────────────────────────────────────

export function writeFile(fleetId: string, path: string, content: string): Promise<WriteResult> {
  return sdk(postApiFleetByIdFsWrite({ path: { id: fleetId }, query: { path }, body: content }))
}

/**
 * Save Univer edits to an xlsx/xlsm by patching the original file server-side
 * (formatting Univer doesn't model is kept). `snapshot` = `workbook.save()`.
 */
export function saveSheet(
  fleetId: string,
  path: string,
  snapshot: Record<string, unknown>,
): Promise<SheetSaveResult> {
  return sdk(
    postApiFleetByIdFsSheetSave({ path: { id: fleetId }, query: { path }, body: snapshot }),
  )
}

export function uploadFile(fleetId: string, dir: string, file: File): Promise<UploadResult> {
  return sdk(
    postApiFleetByIdFsUpload({
      path: { id: fleetId },
      query: { path: dir, name: file.name },
      body: file,
    }),
  )
}

export function uploadUnique(
  fleetId: string,
  dir: string,
  file: File,
): Promise<UploadUniqueResult> {
  return sdk(
    postApiFleetByIdFsUploadUnique({
      path: { id: fleetId },
      query: { path: dir, name: file.name },
      body: file,
    }),
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
  const url = URL.createObjectURL(blob)
  const a = document.createElement("a")
  a.href = url
  a.download = filename
  a.click()
  // Defer the revoke: revoking synchronously after click() can race the
  // browser's own read of the blob and abort the download in some browsers
  // (same fix downloadCaCert applies in maint.ts). 10s is comfortably past the
  // navigation the click kicks off.
  window.setTimeout(() => URL.revokeObjectURL(url), 10_000)
}
