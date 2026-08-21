import { useMemo } from "react"
import { themeIcons } from "seti-icons"

/**
 * The exact file-icon bank VS Code ships out of the box — the **Seti** theme
 * (Jesse Weed's seti-ui), served through the `seti-icons` package. Replaces the
 * bespoke macOS glyphs ({@link FileIcon}) in the explorer tree and the tab
 * strip so a file wears the same icon the user sees in their editor (T629).
 *
 * WHY A CSS MASK, NOT A RAW-HTML INJECTION. `seti-icons` hands back the
 * icon as an SVG *string*, which would normally mean injecting raw markup — a
 * `react/no-danger` + `no-unsanitized` violation that would cost a scoped-off
 * eslint exception and a hash-chain update. Seti icons are single-colour by
 * construction (the theme assigns ONE colour per icon, which is exactly what
 * {@link themeIcons} resolves), so the glyph reproduces losslessly as a
 * monochrome MASK tinted with that colour. That keeps the render a plain,
 * typed inline style — no raw HTML, no suppression.
 *
 * THE PALETTE IS VS CODE'S, not the package default. Bare `getIcon` returns the
 * colour as a Seti keyword (`"blue"`, `"grey"`, …); {@link themeIcons} maps
 * those to hex. The map below is VS Code's own Seti palette (its `theme.less`),
 * so the colours match the editor and not `seti-icons`' Solarized-ish default.
 */
const VSCODE_SETI_PALETTE = {
  blue: "#519aba",
  grey: "#4d5a5e",
  "grey-light": "#6d8086",
  green: "#8dc149",
  orange: "#e37933",
  pink: "#f55385",
  purple: "#a074c4",
  red: "#cc3e44",
  white: "#d4d7d6",
  yellow: "#cbcb41",
  ignore: "#41535b",
} as const

/** Themed resolver, built once at module load (the palette is a constant). */
const getThemedIcon = themeIcons(VSCODE_SETI_PALETTE)

/**
 * A single VS Code / Seti file icon for `name`, rendered as a tinted mask.
 *
 * FOLDERS INTENTIONALLY RENDER NOTHING. In the explorer the open/closed chevron
 * already states a row's folder-ness, so a folder glyph is redundant — the
 * user asked for it gone (T629). Callers still mount this for folders (so the
 * icon column stays a fixed width and names line up); it just returns an empty
 * spacer of the same size.
 */
export function VsCodeFileIcon({
  name,
  isFolder = false,
  size = 16,
}: {
  name: string
  isFolder?: boolean
  size?: number
}) {
  // Memo the file style unconditionally (hooks can't be called under a branch);
  // a folder renders the plain spacer below and never reads it, so computing it
  // for a folder is a cheap no-op we tolerate to keep the hook order stable.
  const style = useMemo(() => {
    const { svg, color } = getThemedIcon(name)
    // seti-icons emits a NAMESPACE-LESS `<svg viewBox=…>`. That is fine inline
    // (React/DOM infers the SVG namespace), but a data-URI loaded through
    // `mask-image` is parsed as a STANDALONE document — without
    // `xmlns="http://www.w3.org/2000/svg"` the browser does not recognise it as
    // SVG and the mask renders NOTHING (every file icon shows blank). Inject the
    // namespace before building the URI.
    const namespaced = svg.includes("xmlns")
      ? svg
      : svg.replace("<svg ", '<svg xmlns="http://www.w3.org/2000/svg" ')
    // The SVG string as a data-URI mask; `encodeURIComponent` (not base64) keeps
    // it human-diffable and is what the `#` / `<` in the markup require to be
    // URL-safe inside `url("…")`.
    const uri = `url("data:image/svg+xml,${encodeURIComponent(namespaced)}")`
    return {
      width: size,
      height: size,
      backgroundColor: color,
      maskImage: uri,
      WebkitMaskImage: uri,
      maskSize: "contain",
      WebkitMaskSize: "contain",
      maskRepeat: "no-repeat",
      WebkitMaskRepeat: "no-repeat",
      maskPosition: "center",
      WebkitMaskPosition: "center",
    } as const
  }, [name, size])

  return (
    <span
      aria-hidden
      className="inline-block shrink-0"
      style={isFolder ? { width: size, height: size } : style}
    />
  )
}
