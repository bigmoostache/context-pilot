import type { FinderNode } from "@/lib/types"

/**
 * File kinds whose content is plain text and can be fetched + rendered live —
 * markdown gets the rich GFM renderer, the rest a preformatted block. Lives in
 * its own module (not beside the preview components) so importing it never
 * breaks Fast Refresh.
 */
export const TEXT_KINDS = new Set<FinderNode["kind"]>(["markdown", "code", "json", "doc"])
