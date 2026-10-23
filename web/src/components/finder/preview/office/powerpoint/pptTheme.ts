/**
 * PowerPoint for the web palette and geometry, measured in the live editor
 * iframe at 1600×900: title bar 48, tab row 36, single-line ribbon 40 inset
 * 8px as a white card, thumbnail pane 256, status bar 23.
 * Segoe UI is Microsoft's font: we name it first and fall back to the system UI.
 */
export const PPT = {
  font: '"Segoe UI", "Segoe UI Web (West European)", system-ui, -apple-system, sans-serif',
  chrome: "#f5f5f5",
  surface: "#ffffff",
  search: "#fafafa",
  ink: "#242424",
  inkTab: "#424242",
  inkMuted: "#707070",
  inkStatus: "#616161",
  inkDisabled: "#bdbdbd",
  brand: "#c43e1c",
  hover: "#ebebeb",
  pressed: "#d6d6d6",
  line: "#e0e0e0",
  shadow: "0 0 2px rgba(0,0,0,0.12), 0 1px 2px rgba(0,0,0,0.14)",
  titleH: 48,
  tabsH: 36,
  ribbonH: 40,
  statusH: 23,
} as const

/** Ribbon tab labels, in PowerPoint's order. Only Home carries commands here. */
export const PPT_TABS = [
  "Home",
  "Insert",
  "Draw",
  "Design",
  "Transitions",
  "Animations",
  "Slide Show",
  "Review",
  "View",
  "Help",
] as const
