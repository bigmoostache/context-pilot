import type { ReactNode } from "react"
import { Grip, Search, Settings, StickyNote } from "lucide-react"
import { PPT } from "./pptTheme"

/**
 * PowerPoint for the web frame around the editor: 48px title bar (app
 * launcher, file name, centred search, Share) and 23px status bar. The
 * editor itself, with PptRibbon as its toolbar, goes in `children`.
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
      <TitleBar fileName={fileName} />
      <div style={{ flex: 1, minHeight: 0 }}>{children}</div>
      <StatusBar />
    </div>
  )
}

function TitleBar({ fileName }: { fileName: string }) {
  return (
    <div
      style={{
        height: PPT.titleH,
        flex: "none",
        position: "relative",
        display: "flex",
        alignItems: "center",
      }}
    >
      <span style={square(48)}>
        <Grip size={20} />
      </span>
      <span style={{ fontSize: 16, fontWeight: 600, color: PPT.brand, padding: "0 12px 0 4px" }}>
        PowerPoint
      </span>
      <span style={{ fontSize: 14, whiteSpace: "nowrap" }}>{stripExt(fileName)}</span>
      <div
        style={{
          position: "absolute",
          left: "50%",
          top: 8,
          width: 450,
          height: 32,
          transform: "translateX(-50%)",
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 12px",
          background: PPT.search,
          borderRadius: 4,
          boxShadow: PPT.shadow,
          fontSize: 14,
          color: PPT.inkMuted,
          boxSizing: "border-box",
        }}
      >
        <Search size={16} />
        Search
      </div>
      <span style={{ flex: 1 }} />
      <span style={square(48)}>
        <Settings size={18} />
      </span>
      <span
        style={{
          ...square(32),
          margin: "0 8px",
          borderRadius: "50%",
          background: PPT.brand,
          color: PPT.surface,
          fontSize: 12,
          fontWeight: 600,
        }}
      >
        CP
      </span>
    </div>
  )
}

function StatusBar() {
  return (
    <div
      style={{
        height: PPT.statusH,
        flex: "none",
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "0 12px",
        fontSize: 12,
        color: PPT.inkStatus,
      }}
    >
      <span>English (U.S.)</span>
      <span style={{ flex: 1 }} />
      <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
        <StickyNote size={14} />
        Notes
      </span>
    </div>
  )
}

function square(size: number) {
  return {
    width: size,
    height: size,
    flex: "none",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
  }
}

function stripExt(name: string) {
  return name.replace(/\.pptx$/i, "")
}
