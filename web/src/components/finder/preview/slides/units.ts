/**
 * Unit helpers for the pptx renderer. The backend ships geometry in EMU
 * (914 400 per inch, 12 700 per point) and font sizes in hundredths of a
 * point; the deck is drawn at `scale` CSS px per EMU so it fits its pane.
 */

/** EMU per typographic point. */
export const EMU_PER_PT = 12_700

/** EMU → CSS px at `scale`. */
export function px(emu: number, scale: number): number {
  return emu * scale
}

/** Hundredths of a point → CSS px at `scale`. */
export function ptPx(hundredths: number, scale: number): number {
  return (hundredths / 100) * EMU_PER_PT * scale
}

/** Preset dash name → SVG dash pattern, in multiples of the stroke width. */
const DASHES: Record<string, number[]> = {
  dot: [1, 1],
  sysDot: [1, 1],
  sysDash: [3, 1],
  dash: [4, 3],
  dashDot: [4, 3, 1, 3],
  lgDash: [8, 3],
  lgDashDot: [8, 3, 1, 3],
  lgDashDotDot: [8, 3, 1, 3, 1, 3],
  sysDashDot: [3, 1, 1, 1],
  sysDashDotDot: [3, 1, 1, 1, 1, 1],
}

/** SVG `stroke-dasharray` for a preset dash at stroke `width`. */
export function dashArray(dash: string | undefined, width: number): string | undefined {
  const pattern = dash ? DASHES[dash] : undefined
  return pattern?.map((n) => n * Math.max(width, 1)).join(" ")
}

/** DrawingML paragraph alignment → CSS `text-align`. */
export function textAlign(align: string): "left" | "center" | "right" | "justify" {
  if (align === "ctr") return "center"
  if (align === "r") return "right"
  if (align === "just" || align === "dist") return "justify"
  return "left"
}

/** Vertical anchor → flex `justify-content`. */
export function anchorJustify(anchor: string): "flex-start" | "center" | "flex-end" {
  if (anchor === "ctr") return "center"
  if (anchor === "b") return "flex-end"
  return "flex-start"
}
