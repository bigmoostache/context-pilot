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
  statusLine: "#f0efef",
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

/** Per-app geometry in CSS px (see {@link officeVars} for the `--of-*` names). */
export interface AppGeometry {
  /** Status bar height. */
  statusH: number
  /** Ribbon strip height (classic two-row or single-line card). */
  ribbonH: number
  /** Tab button height, from the top of the 36 px tab row. */
  tabH: number
  /** Tab side padding; the underline spans the label between them. */
  tabPad: number
  /** Underline thickness and its gap above the tab's bottom edge. */
  tabLine: number
  tabLineBottom: number
  /** Small ribbon button (and menu opener) size; combo box height. */
  btnW: number
  btnH: number
  comboH: number
  /** Combo box chevron width. */
  comboChevW: number
  /** Split button halves: main action and menu chevron. */
  splitW: number
  splitChevW: number
  /** Menu-only button: side padding and icon-to-chevron gap. */
  dropPad: number
  dropGap: number
}

const SINGLE_LINE = {
  btnW: 32,
  btnH: 32,
  comboH: 28,
  comboChevW: 20,
  splitW: 28,
  splitChevW: 16,
  dropPad: 6,
  dropGap: 4,
} as const

export const HEIGHTS: Record<OfficeApp, AppGeometry> = {
  word: {
    statusH: 24,
    ribbonH: 100,
    tabH: 31,
    tabPad: 12,
    tabLine: 2,
    tabLineBottom: 1,
    btnW: 28,
    btnH: 24,
    comboH: 24,
    comboChevW: 24,
    splitW: 26,
    splitChevW: 24,
    dropPad: 3,
    dropGap: 0,
  },
  excel: {
    statusH: 23,
    ribbonH: 40,
    tabH: 36,
    tabPad: 10,
    tabLine: 3,
    tabLineBottom: 4,
    ...SINGLE_LINE,
  },
  powerpoint: {
    statusH: 23,
    ribbonH: 40,
    tabH: 32,
    tabPad: 10,
    tabLine: 3,
    tabLineBottom: 1,
    ...SINGLE_LINE,
  },
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
    "--of-status-line": t.statusLine,
    "--of-card-shadow": t.cardShadow,
    "--of-accent": ACCENT[app],
    "--of-title-h": `${String(FLUENT.titleH)}px`,
    "--of-tabs-h": `${String(FLUENT.tabsH)}px`,
    "--of-radius": `${String(FLUENT.radius)}px`,
    "--of-status-h": `${String(h.statusH)}px`,
    "--of-ribbon-h": `${String(h.ribbonH)}px`,
    "--of-tab-h": `${String(h.tabH)}px`,
    "--of-tab-pad": `${String(h.tabPad)}px`,
    "--of-tab-line": `${String(h.tabLine)}px`,
    "--of-tab-line-bottom": `${String(h.tabLineBottom)}px`,
    "--of-btn-w": `${String(h.btnW)}px`,
    "--of-btn-h": `${String(h.btnH)}px`,
    "--of-combo-h": `${String(h.comboH)}px`,
    "--of-combo-chev-w": `${String(h.comboChevW)}px`,
    "--of-split-w": `${String(h.splitW)}px`,
    "--of-split-chev-w": `${String(h.splitChevW)}px`,
    "--of-drop-pad": `${String(h.dropPad)}px`,
    "--of-drop-gap": `${String(h.dropGap)}px`,
    fontFamily: t.font,
  } as CSSProperties
}
