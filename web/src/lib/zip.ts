import { zip as fflateZip } from "fflate"

// This module owns the ONLY dependency on fflate. It was split out of
// `utils.ts` (L23) so that importing `cn` — pulled in by nearly every component
// — no longer drags the fflate compressor into that module's graph, keeping it
// tree-shakeable / lazy-loadable.

// ── Upload guard-rails (M8) ────────────────────────────────────────────
//
// Attachment paths (composer picker, paste, OS drop) had no size/count limits,
// and a dropped folder was zipped entirely in RAM — a multi-GB drop froze then
// OOM'd the tab. These bounds are enforced at the upload sinks.

/** Max bytes per attached file / archive. Matches the backend transport's
 *  `MAX_BODY` (32 MiB) — a larger body is silently truncated server-side, so we
 *  reject it client-side with a clear message instead. */
export const MAX_ATTACHMENT_BYTES = 32 * 1024 * 1024
/** Max number of files accepted in a single picker / paste batch. */
export const MAX_ATTACHMENT_COUNT = 25
/** Refuse to zip a drop larger than this in the browser (the whole archive is
 *  built in memory via fflate) — the guard that prevents the OOM. */
export const MAX_DROP_TOTAL_BYTES = 256 * 1024 * 1024

/** Human MB label for a byte count, for limit messages. */
export function formatMB(bytes: number): string {
  return `${Math.round(bytes / (1024 * 1024))} MB`
}

// ── Client-side zip-on-drop (T367) ────────────────────────────────────
//
// Bundle the file(s) a user drops onto the thread conversation into a SINGLE
// `.zip` (built entirely in the browser via fflate) before they're uploaded, so
// what lands in the realm is one compressed archive instead of N loose files.
// Only the drag-and-drop path uses this; the paperclip picker still uploads raw
// files unchanged.

/**
 * De-duplicate an entry name within a zip: if `name` is already a key in `taken`,
 * append ` (1)`, ` (2)`… before the extension until it's unique. Two files
 * dragged from different folders can share a basename — without this the second
 * would silently overwrite the first inside the archive.
 */
function uniqueZipEntry(taken: Record<string, unknown>, name: string): string {
  if (!(name in taken)) return name
  const dot = name.lastIndexOf(".")
  const stem = dot > 0 ? name.slice(0, dot) : name
  const ext = dot > 0 ? name.slice(dot) : ""
  for (let i = 1; ; i++) {
    const candidate = `${stem} (${i})${ext}`
    if (!(candidate in taken)) return candidate
  }
}

/**
 * Zip the dropped `files` into one archive `File`, built client-side with
 * fflate (DEFLATE, level 6). The archive is named after the lone file when a
 * single one is dropped (`report.pdf` → `report.pdf.zip`, matching the macOS
 * Finder "Compress" convention) or `dropped-<n>-files.zip` for several. Each
 * entry keeps its original filename (de-duplicated on collision). Rejects if
 * fflate fails or a file can't be read.
 */
export function zipFiles(files: File[]): Promise<File> {
  return zipDropped(files.map((file) => ({ file, path: file.name })))
}

// ── Folder-aware drop extraction (T471) ───────────────────────────────
//
// `dataTransfer.files` does NOT recurse into dropped folders — a folder drop
// yields a single unreadable pseudo-`File`, which then uploads as a failed /
// empty request (the "CORS request did not succeed, status null" a folder drop
// produced). The HTML5 Entry API (`webkitGetAsEntry`) DOES expose the directory
// tree, so we walk it, collect every real file with its folder-relative path,
// and hand back a flat list the caller zips into ONE archive (structure
// preserved) for a single upload — instead of a burst of per-file requests.

/** A file pulled from a drop, tagged with its path relative to the drop root. */
export interface DroppedFile {
  file: File
  /** e.g. `report/q1/data.csv` for a file inside a dropped `report` folder. */
  path: string
}

/** Drain a directory reader fully: `readEntries` is paginated (≈100 entries per
 *  call) and must be pumped until it returns an empty batch. */
function readAllEntries(reader: FileSystemDirectoryReader): Promise<FileSystemEntry[]> {
  return new Promise((resolve, reject) => {
    const acc: FileSystemEntry[] = []
    const pump = () =>
      reader.readEntries((batch) => {
        if (batch.length === 0) {
          resolve(acc)
        } else {
          acc.push(...batch)
          pump()
        }
      }, reject)
    pump()
  })
}

/** Recursively collect files under a filesystem entry, prefixing each with its
 *  path relative to the drop root. */
async function walkEntry(entry: FileSystemEntry, prefix: string): Promise<DroppedFile[]> {
  if (entry.isFile) {
    const fileEntry = entry as FileSystemFileEntry
    const file = await new Promise<File>((res, rej) => fileEntry.file(res, rej))
    return [{ file, path: prefix + entry.name }]
  }
  if (entry.isDirectory) {
    const dirEntry = entry as FileSystemDirectoryEntry
    const children = await readAllEntries(dirEntry.createReader())
    const out: DroppedFile[] = []
    for (const child of children) out.push(...(await walkEntry(child, `${prefix}${entry.name}/`)))
    return out
  }
  return []
}

/**
 * Flatten a drop's `DataTransfer` into every contained file, recursing into
 * folders, each tagged with its folder-relative path. The Entry objects are
 * captured **synchronously** before the first `await` — a `DataTransfer` is
 * neutered the instant the drop handler returns, but the entries it hands out
 * stay valid for later async traversal. Falls back to the flat
 * `dataTransfer.files` when the Entry API is unavailable (older browsers), in
 * which case folder recursion isn't possible.
 */
export async function extractDroppedFiles(dt: DataTransfer): Promise<DroppedFile[]> {
  const entries = Array.from(dt.items)
    .filter((it) => it.kind === "file")
    .map((it) => it.webkitGetAsEntry?.() ?? null)
    .filter((e): e is FileSystemEntry => e !== null)

  if (entries.length === 0) {
    return Array.from(dt.files).map((file) => ({ file, path: file.name }))
  }
  const all: DroppedFile[] = []
  for (const entry of entries) all.push(...(await walkEntry(entry, "")))
  return all
}

/** Pick the archive name: a single file → `<name>.zip`; a single dropped folder
 *  → `<folder>.zip`; anything else → `dropped-<n>-files.zip`. */
function zipName(dropped: DroppedFile[]): string {
  if (dropped.length === 1 && dropped[0]) {
    const only = dropped[0].path
    return `${only.split("/").pop() ?? only}.zip`
  }
  const roots = new Set(dropped.map((d) => d.path.split("/")[0]))
  const [root] = roots
  if (roots.size === 1 && root && dropped.some((d) => d.path.includes("/"))) {
    return `${root}.zip`
  }
  return `dropped-${dropped.length}-files.zip`
}

/**
 * Zip a flat list of {@link DroppedFile}s into one archive `File`, preserving
 * each entry's folder-relative path so a dropped directory tree round-trips
 * intact. Built client-side with fflate (DEFLATE level 6). Rejects if fflate
 * fails or a file can't be read.
 */
export function zipDropped(dropped: DroppedFile[]): Promise<File> {
  return Promise.all(
    dropped.map(async (d) => [d.path, new Uint8Array(await d.file.arrayBuffer())] as const),
  ).then(
    (entries) =>
      new Promise<File>((resolve, reject) => {
        const data: Record<string, Uint8Array> = {}
        for (const [name, bytes] of entries) data[uniqueZipEntry(data, name)] = bytes
        fflateZip(data, { level: 6 }, (err, out) => {
          if (err) {
            reject(err)
            return
          }
          resolve(new File([out], zipName(dropped), { type: "application/zip" }))
        })
      }),
  )
}
