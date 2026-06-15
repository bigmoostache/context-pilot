import {
  File as FileIcon,
  FileArchive,
  FileCode,
  FileImage,
  FileJson,
  FileSpreadsheet,
  FileText,
  FileType,
  Folder,
  Presentation,
  type LucideIcon,
} from "lucide-react"
import type { FinderKind } from "@/lib/types"

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
  binary: { icon: FileIcon, accent: "var(--muted-foreground)", label: "Binary" },
}

/** Soft tinted background derived from a kind's accent. */
export function kindTint(kind: FinderKind, pct = 14): string {
  return `color-mix(in oklab, ${kindMeta[kind].accent} ${pct}%, transparent)`
}
