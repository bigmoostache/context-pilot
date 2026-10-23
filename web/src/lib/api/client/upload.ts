// ── The single upload channel ────────────────────────────────────────
//
// EVERY file upload from the webapp goes through `sendUpload` (Finder,
// composer attachments, drag-drop archives, form file fields, new-agent
// dialog, sheet save-as, avatar). It uses XHR rather than the generated SDK
// (ok:manual) because the Fetch API exposes no upload progress; each in-flight
// upload is tracked in a tiny external store that `UploadToasts` renders
// bottom-right.

import { useSyncExternalStore } from "react"
import { BASE, getToken } from "./index"

/** One tracked upload, as rendered by the toast stack. */
export interface UploadJob {
  id: number
  name: string
  loaded: number
  total: number
  status: "uploading" | "done" | "error"
  error?: string
}

/** How long a finished upload's toast lingers before auto-dismissing. */
const DONE_LINGER_MS = 4000

/** Module-level store: the job list (immutable snapshots) + id counter. */
const store: { jobs: readonly UploadJob[]; nextId: number } = { jobs: [], nextId: 1 }
const listeners = new Set<() => void>()

function setJobs(next: readonly UploadJob[]) {
  store.jobs = next
  for (const l of listeners) l()
}

function patch(id: number, change: Partial<UploadJob>) {
  setJobs(store.jobs.map((j) => (j.id === id ? { ...j, ...change } : j)))
}

/** Remove a toast (user dismiss or auto-dismiss). */
export function dismissUpload(id: number) {
  setJobs(store.jobs.filter((j) => j.id !== id))
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** Live list of tracked uploads (in-flight, just finished, failed). */
export function useUploads(): readonly UploadJob[] {
  return useSyncExternalStore(subscribe, () => store.jobs)
}

/** Best-effort human message from an XHR error response body. */
function failureOf(xhr: XMLHttpRequest): string {
  try {
    const parsed = JSON.parse(xhr.responseText) as { error?: unknown }
    if (typeof parsed.error === "string") return parsed.error
  } catch {
    // non-JSON body — fall through to the status line
  }
  return `${xhr.status} ${xhr.statusText || "upload failed"}`.trim()
}

/**
 * POST `file` as the raw request body to `path` (an `/api/…` route), with
 * progress reported to the toast stack. Resolves the parsed JSON reply; rejects
 * with an `Error` carrying the backend's `{ error }` text (or a network /
 * abort reason). No size cap: the backend streams uploads straight to disk.
 * `quiet` skips the toast (background saves such as office autosave).
 */
export function sendUpload<T>(
  path: string,
  file: Blob,
  name: string,
  { quiet = false }: { quiet?: boolean } = {},
): Promise<T> {
  const id = store.nextId
  store.nextId += 1
  if (!quiet) {
    setJobs([...store.jobs, { id, name, loaded: 0, total: file.size, status: "uploading" }])
  }

  return new Promise<T>((resolve, reject) => {
    const fail = (reason: string) => {
      patch(id, { status: "error", error: reason })
      reject(new Error(`Upload of ${name} failed: ${reason}`))
    }
    const xhr = new XMLHttpRequest()
    xhr.open("POST", `${BASE}${path}`)
    const token = getToken()
    if (token) xhr.setRequestHeader("Authorization", `Bearer ${token}`)
    xhr.setRequestHeader("Content-Type", "application/octet-stream")

    xhr.upload.addEventListener("progress", (e) => {
      patch(id, { loaded: e.loaded, total: e.lengthComputable ? e.total : file.size })
    })
    xhr.addEventListener("load", () => {
      if (xhr.status < 200 || xhr.status >= 300) {
        fail(failureOf(xhr))
        return
      }
      patch(id, { loaded: file.size, status: "done" })
      window.setTimeout(() => dismissUpload(id), DONE_LINGER_MS)
      resolve(JSON.parse(xhr.responseText || "{}") as T)
    })
    xhr.addEventListener("error", () => fail("network error"))
    xhr.addEventListener("abort", () => fail("aborted"))
    xhr.send(file)
  })
}
