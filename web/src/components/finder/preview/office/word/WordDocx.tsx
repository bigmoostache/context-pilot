import type { CSSProperties } from "react"
import { DocxEditor } from "@betteroffice/docx-react"
import { OfficeShell, OfficeTitleBar } from "../fluent/Frame"
import { ACCENT } from "../fluent/tokens"
import { WordRibbon } from "./WordRibbon"
import { WORD_EDITOR_STYLE } from "./wordTheme"

/**
 * The docx editor dressed as Word for the web on the shared Office kit: title
 * bar outside the engine, ribbon (tabs, search, status bar) inside it as its
 * `toolbar`, no ruler, Word's grey canvas. Default export so OfficeEditor can
 * lazy-load it with the engine.
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
    <OfficeShell app="word">
      <OfficeTitleBar title={fileName} logo={<WordLogo />} />
      <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
        <DocxEditor
          documentBuffer={bytes}
          downloadOnSave={false}
          onSave={(buf: ArrayBuffer) => {
            onSave(new Uint8Array(buf))
          }}
          toolbar={<WordRibbon />}
          showRuler={false}
          showZoomControl={false}
          style={WORD_EDITOR_STYLE as CSSProperties}
          className="size-full"
        />
      </div>
    </OfficeShell>
  )
}

/** Plain "W" tile in Word's accent (no Microsoft logo asset). */
function WordLogo() {
  return (
    <span
      aria-hidden
      style={{
        width: 20,
        height: 20,
        borderRadius: 3,
        background: ACCENT.word,
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
  )
}
