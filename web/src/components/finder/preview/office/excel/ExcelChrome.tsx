import type { ReactNode } from "react"
import { EXCEL, EXCEL_TAB } from "./excelTheme"

/**
 * Excel for the web frame around the xlsx editor: ribbon tab row (no title
 * bar: the Finder tab already names the file), then the editor (which hosts the Home ribbon and formula bar), then the
 * status bar.
 */
export function ExcelChrome({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        background: EXCEL.chrome,
        fontFamily: EXCEL.font,
        color: EXCEL.ink,
      }}
    >
      <TabRow />
      <div style={{ position: "relative", flex: 1, minHeight: 0 }}>{children}</div>
      <StatusBar />
    </div>
  )
}

/** Tab row: only Home exists here, so it is a label, not a tab that switches. */
function TabRow() {
  return (
    <div
      style={{
        height: EXCEL.tabsH,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        padding: "0 8px",
        fontSize: 14,
      }}
    >
      <span
        style={{
          marginLeft: 8,
          position: "relative",
          lineHeight: "34px",
          color: EXCEL.ink,
          fontWeight: 600,
        }}
      >
        {EXCEL_TAB}
        <span
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            bottom: 2,
            height: 3,
            borderRadius: 2,
            background: EXCEL.brand,
          }}
        />
      </span>
    </div>
  )
}

/** Excel's status bar strip; empty until it carries live selection aggregates. */
function StatusBar() {
  return (
    <div
      style={{
        height: EXCEL.statusH,
        flexShrink: 0,
        background: EXCEL.surface,
      }}
    />
  )
}
