import { useEffect, useState } from "react"

/** A face handed to `PptxEditor`: the Rust shaper lays out slide text with it. */
export interface PptxFont {
  family: string
  bold: boolean
  italic: boolean
  bytes: Uint8Array
}

/**
 * Bundled metric-compatible families loaded for decks, with the Office names
 * each one stands in for. The shaper matches by family name, so a face is
 * registered under every alias (same bytes, no extra download).
 */
const FAMILIES = new Map<string, string[]>([
  ["Carlito", ["Calibri", "Calibri Light"]],
  ["Liberation Sans", ["Arial", "Helvetica", "Helvetica Neue"]],
  ["Liberation Serif", ["Times New Roman", "Times"]],
])

/** One in-flight/settled load per page; cleared on failure so a retry refetches. */
const cache: { pending: Promise<PptxFont[]> | null } = { pending: null }

/** Fetches the faces once per page (~6 MB, same-origin, in their own chunk). */
function loadPptxFonts(): Promise<PptxFont[]> {
  cache.pending ??= import("@betteroffice/fonts").then(async (fonts) => {
    const faces = fonts.BUNDLED_FONTS.filter((f) => FAMILIES.has(f.family))
    const loaded = await Promise.all(
      faces.map(async (face) => ({
        face,
        bytes: new Uint8Array(await fonts.loadBundledFontBytes(face)),
      })),
    )
    return loaded.flatMap(({ face, bytes }) =>
      [face.family, ...(FAMILIES.get(face.family) ?? [])].map((family) => ({
        family,
        bold: face.weight === 700,
        italic: face.style === "italic",
        bytes,
      })),
    )
  })
  cache.pending.catch(() => {
    cache.pending = null
  })
  return cache.pending
}

/**
 * The deck fonts, `null` while loading. `PptxEditor` refuses to lay out slide
 * text without any font ("No slides"), so the editor mounts only once ready.
 */
export function usePptxFonts(enabled: boolean): { fonts: PptxFont[] | null; error: boolean } {
  const [fonts, setFonts] = useState<PptxFont[] | null>(null)
  const [error, setError] = useState(false)
  useEffect(() => {
    if (!enabled) return
    let live = true
    loadPptxFonts()
      .then((f) => {
        if (live) setFonts(f)
      })
      .catch(() => {
        if (live) setError(true)
      })
    return () => {
      live = false
    }
  }, [enabled])
  return { fonts, error }
}
