import type { ReactNode } from "react"
import { Search } from "lucide-react"
import { EXCEL, EXCEL_TABS } from "./excelTheme"

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
      <span
        style={{
          width: 24,
          height: 24,
          borderRadius: 4,
          background: EXCEL.brand,
          color: "#fff",
          fontWeight: 700,
          fontSize: 13,
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          marginRight: 23,
        }}
      >
        X
      </span>
      <span style={{ fontSize: 14 }}>{title.replace(/\.xlsx$/i, "")}</span>
      <div
        style={{
          position: "absolute",
          left: "50%",
          transform: "translateX(-50%)",
          width: 468,
          height: 32,
          borderRadius: 6,
          background: EXCEL.search,
          border: `1px solid ${EXCEL.line}`,
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 10px",
          color: EXCEL.inkMuted,
          fontSize: 14,
          boxSizing: "border-box",
        }}
      >
        <Search size={16} />
        Search for tools, help, and more (Option + Q)
      </div>
    </div>
  )
}

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
      <span style={{ width: 43, textAlign: "center", color: EXCEL.inkTab }}>File</span>
      {EXCEL_TABS.map((tab) => {
        const active = tab === "Home"
        return (
          <span
            key={tab}
            style={{
              marginLeft: 23,
              position: "relative",
              lineHeight: "34px",
              color: active ? EXCEL.ink : EXCEL.inkTab,
              fontWeight: active ? 600 : 400,
            }}
          >
            {tab}
            {active && (
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
            )}
          </span>
        )
      })}
    </div>
  )
}

function StatusBar() {
  return (
    <div
      style={{
        height: EXCEL.statusH,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        gap: 36,
        padding: "0 32px",
        background: EXCEL.surface,
        color: EXCEL.inkStatus,
        fontSize: 12,
      }}
    >
      <span>Workbook Statistics</span>
    </div>
  )
}
