// ── Thread file-attachment logic (shared, non-forked) ────────────────
//
// The pure upload-message primitives used by BOTH component trees (desktop +
// mobile). Extracted out of `components/threads/fileUpload/helpers.ts` so the
// mobile twin can consume the exact same logic without importing back into the
// desktop tree (the mirror leak-guard forbids `@/components/…` inside
// `mobile-components/`). Presentation forks; this logic stays single-sourced
// here in `@/lib` (design-mobile.md §3.2, architecture rule M141).
//
// Only the COMPONENT-free primitives live here. `uploadToNode` (needs the
// Finder's `kindOf`) and the `CommandSuggestion` UI type stay in the desktop
// helpers module, which re-exports the two symbols below for back-compat.

/**
 * One file attached to a thread via the chat composer. The composer uploads the
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
