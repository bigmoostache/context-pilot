import { useState } from "react"
import {
  XlsxEditor,
  type XlsxCommandStore,
  type XlsxEditorCollaborationOptions,
} from "@betteroffice/xlsx-react"
import { ExcelChrome } from "./ExcelChrome"
import { ExcelHeaderFrame, type HeaderViewport } from "./ExcelHeaders"
import { ExcelToolbar } from "./ExcelRibbon"

/**
 * The xlsx editor dressed as Excel for the web: Excel's frame, the Home ribbon
 * plus formula bar as its toolbar, row/column headers drawn from the engine's
 * viewport, Excel's gridline/selection colours and Windows row heights.
 * Default export so OfficeEditor can lazy-load it with the engine.
 */
export default function ExcelXlsx({
  bytes,
  fileName,
  collaboration,
  onSave,
  onChange,
  onReady,
}: {
  bytes: Uint8Array
  fileName: string
  /** Live session replica (office log); edits then merge instead of reloading. */
  collaboration: XlsxEditorCollaborationOptions
  onSave: (bytes: Uint8Array) => void
  onChange: () => void
  onReady: (api: { readonly commands: XlsxCommandStore }) => void
}) {
  const [viewport, setViewport] = useState<HeaderViewport | null>(null)
  return (
    <ExcelChrome>
      <ExcelHeaderFrame viewport={viewport}>
        <XlsxEditor
          file={bytes}
          fileName={fileName}
          collaboration={collaboration}
          onSave={onSave}
          onChange={onChange}
          onReady={onReady}
          onViewport={setViewport}
          toolbar={<ExcelToolbar />}
          gridlineColor="#e1e1e1"
          selectionColor="#107c41"
          rowMetrics="windows"
          className="size-full"
        />
      </ExcelHeaderFrame>
    </ExcelChrome>
  )
}
