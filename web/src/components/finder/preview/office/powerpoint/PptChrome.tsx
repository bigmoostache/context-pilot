import type { ReactNode } from "react"
import { OfficeShell, OfficeTitleBar } from "../fluent/Frame"

/**
 * PowerPoint for the web frame on the shared Office kit: shell (`--of-*`
 * vars, status slot) and title bar. The editor, with PptRibbon as its
 * toolbar, goes in `children`; the ribbon fills the status slot.
 */
export function PptChrome({ fileName, children }: { fileName: string; children: ReactNode }) {
  return (
    <OfficeShell app="powerpoint">
      <OfficeTitleBar
        title={fileName.replace(/\.pptx$/i, "")}
        logo={
          <span style={{ fontSize: 16, fontWeight: 600, color: "var(--of-accent)" }}>
            PowerPoint
          </span>
        }
      />
      <div style={{ flex: 1, minHeight: 0 }}>{children}</div>
    </OfficeShell>
  )
}
