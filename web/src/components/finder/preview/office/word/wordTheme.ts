/**
 * Word for the web palette and geometry, measured on the live editor at
 * 1440×900 (title bar 48, tab row 36 + 4 gap, classic two-row ribbon card 92,
 * status bar 24). Segoe UI is Microsoft's font: we name it first and fall back
 * to the system UI font.
 */
export const WORD = {
  font: '"Segoe UI", "Segoe UI Web (West European)", system-ui, -apple-system, sans-serif',
  chrome: "#f5f5f5",
  surface: "#ffffff",
  search: "#fafafa",
  ink: "#242424",
  inkTab: "#424242",
  inkMuted: "#616161",
  brand: "#185abd",
  hover: "#ebebeb",
  pressed: "#e0e0e0",
  line: "#e0e0e0",
  pageEdge: "#dadada",
  titleH: 48,
  tabsH: 36,
  ribbonGap: 4,
  ribbonH: 92,
  statusH: 24,
} as const

/**
 * DocxEditor `style` override: the engine paints its canvas with `--doc-bg`
 * (#f8f9fa, set on `.oox-root`), so a parent cannot recolour it; Word's canvas
 * is the same #f5f5f5 as its chrome.
 */
export const WORD_EDITOR_STYLE = {
  "--doc-bg": WORD.chrome,
  "--doc-bg-subtle": WORD.chrome,
} as const

/** Ribbon tab labels after File, in Word's order. Only Home carries commands. */
export const WORD_TABS = [
  "Home",
  "Insert",
  "Layout",
  "References",
  "Review",
  "View",
  "Help",
] as const
