import type { ReactNode } from "react"
import { Grip, MessageSquare, Pencil, Search, Settings, Share2, Video } from "lucide-react"
import { WORD, WORD_TABS } from "./wordTheme"

/**
 * Word for the web frame around the docx editor: title bar, ribbon tab row
 * (with Comments / Catch up / Editing / Share on the right), then the editor
 * (which hosts the Home ribbon and the page canvas), then the status bar.
 */
export function WordChrome({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        background: WORD.chrome,
        fontFamily: WORD.font,
        color: WORD.ink,
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
        height: WORD.titleH,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        position: "relative",
      }}
    >
      <span
        style={{ width: 48, display: "inline-flex", justifyContent: "center", color: "#565656" }}
      >
        <Grip size={18} />
      </span>
      <span
        style={{
          width: 20,
          height: 20,
          margin: "0 26px 0 12px",
          borderRadius: 3,
          background: WORD.brand,
          color: "#fff",
          fontWeight: 700,
          fontSize: 12,
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        W
      </span>
      <span style={{ fontSize: 14 }}>{title.replace(/\.docx$/i, "")}</span>
      <div
        style={{
          position: "absolute",
          left: 540,
          width: 400,
          height: 30,
          borderRadius: 6,
          background: WORD.search,
          border: `1px solid ${WORD.line}`,
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 10px",
          color: WORD.inkMuted,
          fontSize: 14,
          boxSizing: "border-box",
        }}
      >
        <Search size={16} />
        Search for tools, help, and more (Option + Q)
      </div>
      <span
        style={{ marginLeft: "auto", width: 48, display: "inline-flex", justifyContent: "center" }}
      >
        <Settings size={18} />
      </span>
      <span style={{ width: 48, display: "inline-flex", justifyContent: "center" }}>
        <span style={{ width: 28, height: 28, borderRadius: 14, background: "#c7e0f4" }} />
      </span>
    </div>
  )
}

function TabRow() {
  return (
    <div
      style={{
        height: WORD.tabsH,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        padding: "0 8px",
        fontSize: 14,
      }}
    >
      <Tab label="File" />
      {WORD_TABS.map((tab) => (
        <Tab key={tab} label={tab} active={tab === "Home"} />
      ))}
      <span style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
        <Pill icon={<MessageSquare size={16} />} label="Comments" />
        <Pill icon={<Video size={16} />} label="Catch up" />
        <Pill icon={<Pencil size={16} />} label="Editing" />
        <Pill icon={<Share2 size={16} />} label="Share" primary />
      </span>
    </div>
  )
}

function Tab({ label, active = false }: { label: string; active?: boolean }) {
  return (
    <span
      style={{
        position: "relative",
        padding: "0 12px",
        lineHeight: "31px",
        color: active ? WORD.ink : WORD.inkTab,
        fontWeight: active ? 600 : 400,
      }}
    >
      {label}
      {active && (
        <span
          style={{
            position: "absolute",
            left: 12,
            right: 12,
            bottom: 0,
            height: 3,
            borderRadius: 2,
            background: WORD.brand,
          }}
        />
      )}
    </span>
  )
}

function Pill({
  icon,
  label,
  primary = false,
}: {
  icon: ReactNode
  label: string
  primary?: boolean
}) {
  return (
    <span
      style={{
        height: 24,
        padding: "0 10px",
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        borderRadius: 4,
        fontSize: 14,
        color: primary ? "#fff" : WORD.ink,
        background: primary ? WORD.brand : "transparent",
        border: primary ? "none" : `1px solid ${WORD.line}`,
      }}
    >
      {icon}
      {label}
    </span>
  )
}

function StatusBar() {
  return (
    <div
      style={{
        height: WORD.statusH,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        gap: 24,
        padding: "0 16px",
        borderTop: "1px solid #f0efef",
        color: WORD.inkTab,
        fontSize: 12,
      }}
    >
      <span>Page 1</span>
      <span>English (U.S.)</span>
      <span style={{ marginLeft: "auto" }}>100%</span>
    </div>
  )
}
