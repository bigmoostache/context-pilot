import type { FinderNode } from "@/lib/types"

/**
 * File kinds whose content is plain text and can be fetched + rendered live —
 * markdown gets the rich GFM renderer, the rest a preformatted block. Lives in
 * its own module (not beside the preview components) so importing it never
 * breaks Fast Refresh.
 */
export const TEXT_KINDS = new Set<FinderNode["kind"]>(["markdown", "code", "json", "doc"])

/** Extensions the in-browser BetterOffice editors open. */
const OFFICE_KINDS = ["docx", "xlsx", "pptx"] as const

/** An office format the BetterOffice editors handle. */
export type OfficeKind = (typeof OFFICE_KINDS)[number]

/** Office kind from the path's extension, or null when the editors can't open it. */
export function officeKindOf(path: string): OfficeKind | null {
  const ext = path.split(".").pop()?.toLowerCase()
  return OFFICE_KINDS.find((k) => k === ext) ?? null
}
