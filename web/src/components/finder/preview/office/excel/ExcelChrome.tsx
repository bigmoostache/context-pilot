import type { ReactNode } from "react"
import { EXCEL, EXCEL_TAB } from "./excelTheme"

/**
 * Excel for the web frame around the xlsx editor: title bar, ribbon tab row,
 * then the editor (which hosts the Home ribbon and formula bar), then the
 * status bar.
 */
export function ExcelChrome({ title, children }: { title: string; children: ReactNode }) {
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
      <TitleBar title={title} />
      <TabRow />
      <div style={{ position: "relative", flex: 1, minHeight: 0 }}>{children}</div>
      <StatusBar />
    </div>
  )
}

/** Excel's app mark, as the web viewer's `Excel_20` icon draws it (fills resolved from its CSS). */
const LOGO_PATHS: readonly (readonly [string, string])[] = [
  [
    "#1e6824",
    "M307 742q0-95 68-163 67-67 163-67h1305v1280q0 64-45 109-45 45-108 45h-1076q-127 0-217-90-90-90-90-218v-896z",
  ],
  [
    "#47a245",
    "M307 947q0-95 68-163 67-67 163-67h742q-64 0-109 45-45 45-45 108v308q0 64-45 108-45 45-108 45h-359q-127 0-217 90-90 90-90 217v-691z",
  ],
  ["#72d560", "M307 410q0-127 90-218 90-90 217-90h615v615h-615q-127 0-217 90-90 90-90 217v-614z"],
  [
    "#bdea74",
    "M1178 102h512q64 0 108 45 45 45 45 109v307q0 64-45 109-45 45-108 45h-512q-64 0-109-45-45-45-45-109v-307q0-64 45-109 45-45 109-45z",
  ],
  [
    "#0e5734",
    "M717 819h-512q-85 0-145 60-60 60-60 145v512q0 85 60 145 60 60 145 60h512q85 0 145-60 60-60 60-145v-512q0-85-60-145-60-60-145-60z",
  ],
  [
    "#ffffff",
    "M697 1536h-139l-87-163q-5-9-7-14-2-6-4-12h-2q-3 9-6 14-3 5-7 13l-90 162h-130l156-256-145-256h136l77 146q5 9 8 15 4 6 7 16h2q5-10 8-17 3-5 8-16l80-144h130l-148 252 153 260z",
  ],
]

/** 20px mark at x22 of the title bar, title text starting at x63 like the reference. */
function ExcelLogo() {
  return (
    <svg
      aria-hidden
      viewBox="0 0 2048 2048"
      width={20}
      height={20}
      style={{ marginLeft: 6, marginRight: 21, flexShrink: 0 }}
    >
      {LOGO_PATHS.map(([fill, d]) => (
        <path key={fill} fill={fill} d={d} />
      ))}
    </svg>
  )
}

function TitleBar({ title }: { title: string }) {
  return (
    <div
      style={{
        height: EXCEL.titleH,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        position: "relative",
        padding: "0 16px",
      }}
    >
      <ExcelLogo />
      <span style={{ fontSize: 14 }}>{title.replace(/\.xlsx$/i, "")}</span>
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
