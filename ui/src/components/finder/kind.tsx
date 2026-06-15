import {
  File as FileIcon,
  FileArchive,
  FileAudio,
  FileCode,
  FileImage,
  FileJson,
  FileSpreadsheet,
  FileText,
  FileType,
  FileVideo,
  Folder,
  Presentation,
  type LucideIcon,
} from "lucide-react"
import type { FinderKind, FinderTag } from "@/lib/types"

/** Visual identity for each file kind: icon, accent token, human label. */
export const kindMeta: Record<FinderKind, { icon: LucideIcon; accent: string; label: string }> = {
  folder: { icon: Folder, accent: "var(--warn)", label: "Folder" },
  code: { icon: FileCode, accent: "var(--interactive)", label: "Source" },
  doc: { icon: FileText, accent: "var(--signal)", label: "Document" },
  pdf: { icon: FileType, accent: "var(--danger)", label: "PDF" },
  sheet: { icon: FileSpreadsheet, accent: "var(--ok)", label: "Spreadsheet" },
  slides: { icon: Presentation, accent: "var(--warn)", label: "Keynote" },
  image: { icon: FileImage, accent: "var(--interactive)", label: "Image" },
  markdown: { icon: FileText, accent: "var(--signal)", label: "Markdown" },
  json: { icon: FileJson, accent: "var(--ok)", label: "JSON" },
  archive: { icon: FileArchive, accent: "var(--muted-foreground)", label: "Archive" },
  audio: { icon: FileAudio, accent: "var(--signal)", label: "Audio" },
  video: { icon: FileVideo, accent: "var(--interactive)", label: "Video" },
  binary: { icon: FileIcon, accent: "var(--muted-foreground)", label: "Binary" },
}

/** Soft tinted background derived from a kind's accent. */
export function kindTint(kind: FinderKind, pct = 14): string {
  return `color-mix(in oklab, ${kindMeta[kind].accent} ${pct}%, transparent)`
}

/** A subtle two-stop gradient for the big icon chips — gives them depth. */
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
