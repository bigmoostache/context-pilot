// ── Agent file-attachment logic (shared, non-forked) ────────────────
//
// The pure upload-message primitives used by BOTH component trees (desktop +
// mobile). Extracted out of `components/agents/fileUpload/helpers.ts` so the
// mobile twin can consume the exact same logic without importing back into the
// desktop tree (the mirror leak-guard forbids `@/components/…` inside
// `mobile-components/`). Presentation forks; this logic stays single-sourced
// here in `@/lib` (design-mobile.md §3.2, architecture rule M141).
//
// Only the COMPONENT-free primitives live here. `uploadToNode` (needs the
// Finder's `kindOf`) stays in the desktop helpers module, which re-exports the
// symbols below for back-compat.

import type { ClipboardEvent } from "react"

/**
 * One file attached to a agent via the chat composer. The composer uploads the
 * file to the realm's `.uploads/` and embeds these fields into the user message
 * as a ` ```file-upload ` YAML block (one block per file); the conversation view
 * parses the blocks back out and renders each as a clickable `FileUploadChip`.
 */
export interface UploadedFile {
  /** realm-relative stored path, e.g. `.uploads/report (1).pdf` */
  path: string
  /** stored filename */
  name: string
  /** byte count */
  size: number
  /** provenance note, e.g. `uploaded by user at 2026-…` */
  note: string
}

/**
 * Compose a user message body carrying one ` ```file-upload ` YAML block per
 * uploaded file. The conversation renderer extracts these blocks and turns them
 * into clickable preview chips rendered **inline at the block's position**; the
 * agent reads the same YAML as plain context, so it knows which files were
 * attached.
 */
export function buildUploadMessage(files: UploadedFile[]): string {
  return files
    .map((f) =>
      [
        "```file-upload",
        "file:",
        `  path: ${f.path}`,
        `  name: ${f.name}`,
        `  size: ${f.size}`,
        `  note: ${f.note}`,
        "```",
      ].join("\n"),
    )
    .join("\n\n")
}

/** Pasted text at or above this many characters is attached as a `.txt` file
 *  instead of being inlined in the composer. Same value as the TUI's
 *  `LARGE_PASTE_CHARS`. */
export const LARGE_PASTE_CHARS = 2000

/**
 * Turn a large text paste into a `paste-<timestamp>.txt` file ready for the
 * regular attach path (upload to `.uploads/` + staged chip). Returns `null`
 * for pastes below {@link LARGE_PASTE_CHARS}, which stay inline.
 */
export function largePasteFile(text: string): File | null {
  if (text.length < LARGE_PASTE_CHARS) return null
  const stamp = new Date().toISOString().slice(0, 19).replaceAll(/[-:]/g, "").replace("T", "-")
  return new File([text], `paste-${stamp}.txt`, { type: "text/plain" })
}

/**
 * Shared composer `onPaste` handler: pasted images, or text at/above
 * {@link LARGE_PASTE_CHARS}, are attached as files; anything else pastes
 * inline as usual.
 */
export function handleComposerPaste(
  e: ClipboardEvent,
  onAttach: ((files: File[]) => void | Promise<void>) | undefined,
): void {
  if (!onAttach) return
  const images = [...e.clipboardData.items]
    .filter((i) => i.kind === "file" && i.type.startsWith("image/"))
    .map((i) => i.getAsFile())
    .filter((f): f is File => f !== null)
  const pasted = images.length > 0 ? null : largePasteFile(e.clipboardData.getData("text/plain"))
  const files = pasted ? [pasted] : images
  if (files.length === 0) return
  e.preventDefault()
  void onAttach(files)
}

/**
 * A `/command` offered as a composer suggestion bubble (T348/T350). `command`
 * is the literal slash token (e.g. `/clean`); `name` + `description` label the
 * bubble; `body` is the expanded prompt seeded on click (when present).
 *
 * Lives here — beside {@link UploadedFile} — so the composer's two pill families
 * (file attachments + command suggestions) share ONE module and ONE rendered row
 * (`ComposerBubbles`), instead of two divergent ad-hoc blocks that can't coexist.
 */
export interface CommandSuggestion {
  command: string
  name: string
  description: string
  /** the prompt body the `/command` expands to; seeded into the composer on
   *  click. Falls back to the bare `command` literal when absent. */
  body?: string | undefined
}
