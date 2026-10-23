import type { CSSProperties } from "react"

/**
 * The command shape all three BetterOffice engines return from
 * `use{Docx,Xlsx,Pptx}Command(id, args)`. The kit consumes this shape only and
 * never imports an engine. `A` is the command's argument type (`void` = none).
 */
export interface Cmd<V = unknown, A = void> {
  label: string
  state: { enabled: boolean; active?: boolean | undefined; value?: V | undefined }
  execute: (args: A) => unknown
}

export type OfficeApp = "word" | "excel" | "powerpoint"

/**
 * Office for the web (Fluent 2) neutrals, measured on Excel for the web at
 * 1440×900. Segoe UI is Microsoft's font: named first, system UI as fallback.
 */
export const FLUENT = {
  font: '"Segoe UI", "Segoe UI Web (West European)", system-ui, -apple-system, sans-serif',
  chrome: "#f5f5f5",
  surface: "#ffffff",
  search: "#fafafa",
  ink: "#242424",
  inkTab: "#424242",
  inkMuted: "#616161",
  hover: "#ebebeb",
  pressed: "#e0e0e0",
  line: "#e0e0e0",
  cardShadow: "0 0 2px rgba(0,0,0,.12), 0 1px 2px rgba(0,0,0,.14)",
  titleH: 48,
  tabsH: 36,
  radius: 8,
} as const

/** Brand colour per app: tab underline, logo, primary buttons. */
export const ACCENT: Record<OfficeApp, string> = {
  word: "#185abd",
  excel: "#107c41",
  powerpoint: "#c43e1c",
}

/** Office's default theme colours (top row of the picker), RGB hex without `#`. */
export const THEME_COLORS: readonly string[] = [
  "FFFFFF",
  "000000",
  "E8E8E8",
  "0E2841",
  "156082",
  "E97132",
  "196B24",
  "0F9ED5",
  "A02B93",
  "4EA72E",
]

/** Office's standard colours row, RGB hex without `#`. */
export const STANDARD_COLORS: readonly string[] = [
  "C00000",
  "FF0000",
  "FFC000",
  "FFFF00",
  "92D050",
  "00B050",
  "00B0F0",
  "0070C0",
  "002060",
  "7030A0",
]

/** Per-app bar heights in CSS px: status bar, ribbon card (classic two-row or single-line). */
export const HEIGHTS: Record<OfficeApp, { statusH: number; ribbonH: number }> = {
  word: { statusH: 24, ribbonH: 92 },
  excel: { statusH: 23, ribbonH: 40 },
  powerpoint: { statusH: 23, ribbonH: 40 },
}

/** Measured per-app deviations from {@link FLUENT}; code stays shared. */
const OVERRIDES: Record<OfficeApp, Partial<Record<keyof typeof FLUENT, string>>> = {
  word: {},
  excel: { inkMuted: "#707070" },
  powerpoint: {},
}

/**
 * The `--of-*` CSS variables set on the shell root; every kit component reads
 * colours and heights from these, so one app can retheme without new code.
 */
export function officeVars(app: OfficeApp): CSSProperties {
  const t = { ...FLUENT, ...OVERRIDES[app] }
  const h = HEIGHTS[app]
  return {
    "--of-font": t.font,
    "--of-chrome": t.chrome,
    "--of-surface": t.surface,
    "--of-search": t.search,
    "--of-ink": t.ink,
    "--of-ink-tab": t.inkTab,
    "--of-ink-muted": t.inkMuted,
    "--of-hover": t.hover,
    "--of-pressed": t.pressed,
    "--of-line": t.line,
    "--of-card-shadow": t.cardShadow,
    "--of-accent": ACCENT[app],
    "--of-title-h": `${String(FLUENT.titleH)}px`,
    "--of-tabs-h": `${String(FLUENT.tabsH)}px`,
    "--of-radius": `${String(FLUENT.radius)}px`,
    "--of-status-h": `${String(h.statusH)}px`,
    "--of-ribbon-h": `${String(h.ribbonH)}px`,
    fontFamily: t.font,
  } as CSSProperties
}
