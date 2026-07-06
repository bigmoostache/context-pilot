import type { FinderNode } from "@/lib/types"

// Split out of `livePreviews.tsx` so that component file only exports components
// (React Fast Refresh). Consumed by `FinderPreview` to decide which node kinds
// get the live text/markdown preview.

/** File kinds whose content is plain text and can be fetched + rendered live
 *  (markdown gets the rich GFM renderer; the rest a preformatted block). */
export const TEXT_KINDS = new Set<FinderNode["kind"]>(["markdown", "code", "json", "doc"])
