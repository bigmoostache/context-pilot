import type { ReactNode } from "react"
import { OfficeShell } from "../fluent/Frame"

/**
 * Excel for the web frame on the shared Office kit: shell (`--of-*` vars,
 * status slot). No title bar: the Finder tab already names the file. The
 * editor, with ExcelToolbar (tabs, ribbon, formula bar) as its toolbar, goes
 * in `children`; the toolbar fills the status slot.
 */
export function ExcelChrome({ children }: { children: ReactNode }) {
  return (
    <OfficeShell app="excel">
      <div style={{ position: "relative", flex: 1, minHeight: 0 }}>{children}</div>
    </OfficeShell>
  )
}
