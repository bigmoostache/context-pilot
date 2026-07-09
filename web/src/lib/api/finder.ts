// ── Finder filesystem REST endpoints (SDK) ──────────────────────────
//
// All endpoints except rawUrl (URL builder) and downloadFile (binary
// download with Content-Disposition) use the generated SDK.  Types are
// re-exported from generated/types.gen so existing imports keep working.

import type {
  ConversationMsg as GenConversationMsg,
  FinderNode,
  FsPreview,
  MkdirResult,
  MoveResult,
  RenameResult,
  SheetData,
  TrashResult,
  UploadResult,
  UploadUniqueResult,
  WriteResult,
} from "./generated/types.gen"
import {
  getApiAgentByIdConversation,
  getApiAgentByIdFs,
  getApiAgentByIdFsDescriptions,
  getApiAgentByIdFsPreview,
  getApiAgentByIdFsSheet,
  postApiAgentByIdFsMkdir,
  postApiAgentByIdFsMove,
  postApiAgentByIdFsRename,
  postApiAgentByIdFsTrash,
  postApiAgentByIdFsUpload,
  postApiAgentByIdFsUploadUnique,
  postApiAgentByIdFsWrite,
} from "./generated"
import { BASE, getToken, sdk } from "./client"

// ── Type re-exports (preserve import surface) ────────────────────────

export type { FinderNode, FsPreview, SheetData } from "./generated/types.gen"
export type { WriteResult, UploadResult, UploadUniqueResult } from "./generated/types.gen"
export type { MkdirResult, RenameResult, MoveResult, TrashResult } from "./generated/types.gen"
export type { ConversationMsg } from "./generated/types.gen"

// ── GET endpoints (SDK) ──────────────────────────────────────────────

export function fetchFs(agentId: string, path = ""): Promise<FinderNode[]> {
  return sdk(getApiAgentByIdFs({ path: { id: agentId }, query: { path } }))
}

export function fetchDescriptions(agentId: string): Promise<Record<string, string>> {
  return sdk(getApiAgentByIdFsDescriptions({ path: { id: agentId } }))
}

export function fetchFsPreview(agentId: string, path: string): Promise<FsPreview> {
  return sdk(getApiAgentByIdFsPreview({ path: { id: agentId }, query: { path } }))
}

export function fetchSheet(agentId: string, path: string): Promise<SheetData> {
  return sdk(getApiAgentByIdFsSheet({ path: { id: agentId }, query: { path } }))
}

export function fetchConversation(agentId: string): Promise<GenConversationMsg[]> {
  return sdk(getApiAgentByIdConversation({ path: { id: agentId } }))
}

// ── POST endpoints (SDK) ─────────────────────────────────────────────

export function writeFile(agentId: string, path: string, content: string): Promise<WriteResult> {
  return sdk(postApiAgentByIdFsWrite({ path: { id: agentId }, query: { path }, body: content }))
}

export function uploadFile(agentId: string, dir: string, file: File): Promise<UploadResult> {
  return sdk(
    postApiAgentByIdFsUpload({
      path: { id: agentId },
      query: { path: dir, name: file.name },
      body: file,
    }),
  )
}

export function uploadUnique(
  agentId: string,
  dir: string,
  file: File,
): Promise<UploadUniqueResult> {
  return sdk(
    postApiAgentByIdFsUploadUnique({
      path: { id: agentId },
      query: { path: dir, name: file.name },
      body: file,
    }),
  )
}

export function createFolder(agentId: string, dir: string, name: string): Promise<MkdirResult> {
  return sdk(postApiAgentByIdFsMkdir({ path: { id: agentId }, query: { path: dir, name } }))
}

export function renameItem(agentId: string, path: string, newName: string): Promise<RenameResult> {
  return sdk(postApiAgentByIdFsRename({ path: { id: agentId }, query: { path, name: newName } }))
}

export function moveItems(agentId: string, items: string[], dest: string): Promise<MoveResult> {
  return sdk(postApiAgentByIdFsMove({ path: { id: agentId }, body: { items, dest } }))
}

export function trashItems(agentId: string, items: string[]): Promise<TrashResult> {
  return sdk(postApiAgentByIdFsTrash({ path: { id: agentId }, body: { items } }))
}

// ── Manual endpoints (irreducible — URL builder + binary download) ───

/** Build the URL that serves a realm file's raw bytes inline (no download
 *  prompt). `fs/raw` is NOT a public route, so an `<img src>` / `<object data>`
 *  pointed straight at this URL can't carry the `Authorization` header and 401s
 *  once auth is on (C2). Kept only for the rare same-origin/no-auth caller. */
export function rawUrl(agentId: string, path: string): string {
  return `${BASE}/api/agent/${agentId}/fs/raw?path=${encodeURIComponent(path)}`
}

/** Trigger a browser download for a file in the agent's realm. */
export async function downloadFile(agentId: string, path: string): Promise<void> {
  const headers: Record<string, string> = {}
  const token = getToken()
  if (token) headers["Authorization"] = `Bearer ${token}`

  const res = await fetch(
    // ok:manual — binary blob download, irreducible
    `${BASE}/api/agent/${agentId}/fs/download?path=${encodeURIComponent(path)}`,
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
