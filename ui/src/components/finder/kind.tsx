import type { FinderKind, FinderTag } from "@/lib/types"

/**
 * Visual identity for each file kind: accent token + human label.
 *
 * The actual file/folder *icons* are no longer monochrome lucide glyphs — they
 * are full-color, macOS-faithful SVGs rendered by `<FileIcon>` in `macIcons.tsx`
 * (T27). What remains here is the non-icon metadata every view still needs: the
 * `accent` token (for tints, focus, small swatches) and the `label` ("Source",
 * "PDF"…) shown in list/columns/info surfaces.
 */
export const kindMeta: Record<FinderKind, { accent: string; label: string }> = {
  folder: { accent: "var(--warn)", label: "Folder" },
  code: { accent: "var(--interactive)", label: "Source" },
  doc: { accent: "var(--signal)", label: "Document" },
  pdf: { accent: "var(--danger)", label: "PDF" },
  sheet: { accent: "var(--ok)", label: "Spreadsheet" },
  slides: { accent: "var(--warn)", label: "Keynote" },
  image: { accent: "var(--interactive)", label: "Image" },
  markdown: { accent: "var(--signal)", label: "Markdown" },
  json: { accent: "var(--ok)", label: "JSON" },
  archive: { accent: "var(--muted-foreground)", label: "Archive" },
  audio: { accent: "var(--signal)", label: "Audio" },
  video: { accent: "var(--interactive)", label: "Video" },
  binary: { accent: "var(--muted-foreground)", label: "Binary" },
}

/** Soft tinted background derived from a kind's accent. */
export function kindTint(kind: FinderKind, pct = 14): string {
  return `color-mix(in oklab, ${kindMeta[kind].accent} ${pct}%, transparent)`
}

/** A subtle two-stop gradient — kept for non-icon decorative chips. */
export function kindGradient(kind: FinderKind): string {
  const a = kindMeta[kind].accent
  return `linear-gradient(160deg, color-mix(in oklab, ${a} 26%, transparent), color-mix(in oklab, ${a} 9%, transparent))`
}

/** macOS finder tag colors. */
export const TAG_META: Record<FinderTag, { color: string; label: string }> = {
  red: { color: "#ff5f57", label: "Red" },
  orange: { color: "#ff9f0a", label: "Orange" },
  yellow: { color: "#ffd60a", label: "Yellow" },
  green: { color: "#30d158", label: "Green" },
  blue: { color: "#0a84ff", label: "Blue" },
  purple: { color: "#bf5af2", label: "Purple" },
  gray: { color: "#98989d", label: "Gray" },
}

/** Extension helper for `<FileIcon ext>` — bare uppercase ext from a filename. */
export function extOf(name: string): string | undefined {
  const i = name.lastIndexOf(".")
  return i > 0 ? name.slice(i + 1) : undefined
}
