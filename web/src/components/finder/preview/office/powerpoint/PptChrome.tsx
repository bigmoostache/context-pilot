import type { ReactNode } from "react"
import { PPT } from "./pptTheme"

/**
 * PowerPoint for the web frame around the editor: 48px title bar (app name,
 * file name) and 23px status bar. No dead controls: Microsoft cloud items
 * (waffle, search, settings, account, Share) are left out because nothing
 * backs them. The editor, with PptRibbon as its toolbar, goes in `children`.
 */
export function PptChrome({ fileName, children }: { fileName: string; children: ReactNode }) {
  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: PPT.chrome,
        fontFamily: PPT.font,
        color: PPT.ink,
      }}
    >
      <div
        style={{
          height: PPT.titleH,
          flex: "none",
          display: "flex",
          alignItems: "center",
          padding: "0 16px",
        }}
      >
        <span style={{ fontSize: 16, fontWeight: 600, color: PPT.brand, paddingRight: 12 }}>
          PowerPoint
        </span>
        <span style={{ fontSize: 14, whiteSpace: "nowrap" }}>{stripExt(fileName)}</span>
      </div>
      <div style={{ flex: 1, minHeight: 0 }}>{children}</div>
      <div
        style={{
          height: PPT.statusH,
          flex: "none",
          display: "flex",
          alignItems: "center",
          padding: "0 12px",
          fontSize: 12,
          color: PPT.inkStatus,
        }}
      >
        English (U.S.)
      </div>
    </div>
  )
}

function stripExt(name: string) {
  return name.replace(/\.pptx$/i, "")
}
