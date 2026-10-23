import type { CSSProperties } from "react"
import { DocxEditor } from "@betteroffice/docx-react"
import { WordChrome } from "./WordChrome"
import { WordRibbon } from "./WordRibbon"
import { WORD_EDITOR_STYLE } from "./wordTheme"

/**
 * The docx editor dressed as Word for the web: Word's frame around it, the
 * Home ribbon as its toolbar (inside the command context), no ruler, Word's
 * grey canvas. Default export so OfficeEditor can lazy-load it with the engine.
 */
export default function WordDocx({
  bytes,
  fileName,
  onSave,
}: {
  bytes: Uint8Array
  fileName: string
  onSave: (bytes: Uint8Array) => void
}) {
  return (
    <WordChrome title={fileName}>
      <DocxEditor
        documentBuffer={bytes}
        downloadOnSave={false}
        onSave={(buf: ArrayBuffer) => {
          onSave(new Uint8Array(buf))
        }}
        toolbar={<WordRibbon />}
        showRuler={false}
        style={WORD_EDITOR_STYLE as CSSProperties}
        className="size-full"
      />
    </WordChrome>
  )
}
