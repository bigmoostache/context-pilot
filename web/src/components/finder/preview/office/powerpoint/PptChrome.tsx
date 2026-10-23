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
      <div className="ppt-body" style={{ flex: 1, minHeight: 0 }}>
        <style href="ppt-body" precedence="default">
          {ENGINE_AREAS}
        </style>
        {children}
      </div>
    </OfficeShell>
  )
}

/**
 * The engine styles its thumbnail strip and slide stage inline (no CSS vars),
 * so they are matched structurally: the strip wraps the slides `aside`, the
 * stage wraps the slide canvas. Values are measured on PowerPoint for the web.
 * Matches the DOM of @betteroffice/pptx-react 0.2.1-cp.20261023.1: recheck on pin bumps.
 */
const ENGINE_AREAS = `
.ppt-body div:has(> aside[aria-label]) { width: 256px !important; background: #f5f5f5 !important; }
.ppt-body div:has(> canvas[data-testid="pptx-slide-canvas"]) { background: #f5f5f5 !important; }
`
