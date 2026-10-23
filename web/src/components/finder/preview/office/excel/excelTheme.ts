/**
 * Excel for the web palette and geometry, measured on the Office Online viewer
 * at 1440×900 (title bar 48, tab row 36, single-line ribbon 40, status bar 23).
 * Segoe UI is Microsoft's font: we name it first and fall back to the system UI.
 */
export const EXCEL = {
  font: '"Segoe UI", "Segoe UI Web (West European)", system-ui, -apple-system, sans-serif',
  chrome: "#f5f5f5",
  surface: "#ffffff",
  search: "#fafafa",
  ink: "#242424",
  inkTab: "#424242",
  inkMuted: "#707070",
  inkStatus: "#444444",
  brand: "#107c41",
  hover: "#ebebeb",
  pressed: "#e0e0e0",
  line: "#e0e0e0",
  titleH: 48,
  tabsH: 36,
  ribbonH: 40,
  formulaH: 32,
  statusH: 23,
} as const

/** Ribbon tab labels, in Excel's order. Only Home carries commands here. */
export const EXCEL_TABS = [
  "Home",
  "Insert",
  "Share",
  "Page Layout",
  "Formulas",
  "Data",
  "Review",
  "View",
  "Help",
  "Draw",
] as const
