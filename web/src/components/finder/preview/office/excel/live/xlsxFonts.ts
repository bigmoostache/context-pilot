import { useEffect, useState } from "react"

/**
 * Office families workbooks name, each mapped to its bundled metric-compatible
 * face. The canvas paints with CSS font names, so a missing Calibri would fall
 * back to the platform sans-serif (Helvetica on macOS: heavier, taller caps).
 */
const ALIASES: readonly (readonly [office: string, bundled: string])[] = [
  ["Calibri", "Carlito"],
  ["Cambria", "Caladea"],
  ["Arial", "Liberation Sans"],
  ["Helvetica", "Liberation Sans"],
  ["Times New Roman", "Liberation Serif"],
]

/** Is `family` installed on this machine? A real face always wins over the alias. */
async function installed(family: string): Promise<boolean> {
  try {
    await new FontFace(family, `local("${family}")`).load()
    return true
  } catch {
    return false
  }
}

/**
 * Registers the bundled face under every Office name that isn't installed
 * locally (document-wide FontFaces, same-origin, in their own chunk).
 */
async function registerMissing(): Promise<void> {
  const fonts = await import("@betteroffice/fonts")
  const flags = await Promise.all(ALIASES.map(async ([office]) => installed(office)))
  const missing = ALIASES.filter((_, i) => flags[i] !== true)
  await Promise.all(
    missing.flatMap(([office, bundled]) =>
      fonts.BUNDLED_FONTS.filter((f) => f.family === bundled).map(async (face) =>
        fonts.registerBundledFontFace(face, office),
      ),
    ),
  )
}

/** One load per page; cleared on failure so the next workbook retries. */
const cache: { pending: Promise<void> | null } = { pending: null }

/** Loads the workbook fonts once per page. */
export async function loadXlsxFonts(): Promise<void> {
  cache.pending ??= registerMissing()
  try {
    await cache.pending
  } catch (error) {
    cache.pending = null
    throw error
  }
}

/** Settles when the fonts are registered or failed to load; never rejects. */
async function settleFonts(): Promise<void> {
  try {
    await loadXlsxFonts()
  } catch {
    // The editor then paints with the fallback face.
  }
}

/**
 * True once the workbook fonts are registered, or once loading failed: the
 * editor then mounts on the fallback face rather than not at all.
 */
export function useXlsxFontsReady(): boolean {
  const [ready, setReady] = useState(false)
  useEffect(() => {
    let live = true
    void settleFonts().finally(() => {
      if (live) setReady(true)
    })
    return () => {
      live = false
    }
  }, [])
  return ready
}
