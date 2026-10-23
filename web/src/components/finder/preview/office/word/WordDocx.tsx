import { useRef, useState } from "react"
import type { CSSProperties } from "react"
import { DocxEditor } from "@betteroffice/docx-react"
import type { DocxEditorRef } from "@betteroffice/docx-react"
import { OfficeShell, OfficeTitleBar } from "../fluent/Frame"
import { ACCENT } from "../fluent/tokens"
import { WordRibbon } from "./WordRibbon"
import { WordEditorContext } from "./wordCmd"
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
  onRetry,
}: {
  bytes: Uint8Array
  fileName: string
  onSave: (bytes: Uint8Array) => void
  onRetry: () => void
}) {
  const [error, setError] = useState<string | null>(null)
  const notZip = !isZip(bytes)
  const message = notZip ? "The file is not a Word document (no .docx container)." : error
  return (
    <OfficeShell app="word">
      <OfficeTitleBar title={fileName} logo={<WordLogo />} />
      {message === null ? (
        <EditorArea bytes={bytes} onSave={onSave} onError={setError} />
      ) : (
        <OpenError
          message={message}
          onRetry={() => {
            setError(null)
            onRetry()
          }}
        />
      )}
    </OfficeShell>
  )
}

/** A .docx is a zip: it starts with the local-file-header magic "PK\x03\x04". */
const ZIP_MAGIC = [80, 75, 3, 4] as const

function isZip(bytes: Uint8Array) {
  return ZIP_MAGIC.every((b, i) => bytes[i] === b)
}

/**
 * Word for the web centres the page in the canvas minus its right scrollbar
 * gutter (18 px here puts the page edge on Word's pixel). The engine centres across the full width with a symmetric
 * gutter, which sits the page 8 px right of Word's. Word's page edge is a 1 px
 * #dadada hairline, not the engine's soft shadow.
 */
const CANVAS_CSS =
  ".word-canvas .docx-editor__scroll-container{scrollbar-gutter:auto;padding-right:18px;margin-top:-8px}" +
  ".word-canvas .canvas-page>canvas{box-shadow:0 0 0 1px #dadada!important}"

function EditorArea({
  bytes,
  onSave,
  onError,
}: {
  bytes: Uint8Array
  onSave: (bytes: Uint8Array) => void
  onError: (message: string) => void
}) {
  const editorRef = useRef<DocxEditorRef>(null)
  return (
    <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
      <WordEditorContext value={editorRef}>
        <DocxEditor
          ref={editorRef}
          documentBuffer={bytes}
          downloadOnSave={false}
          onSave={(buf: ArrayBuffer) => {
            onSave(new Uint8Array(buf))
          }}
          onError={(e: Error) => {
            onError(e.message || "The document could not be read.")
          }}
          toolbar={<WordRibbon />}
          showRuler={false}
          showZoomControl={false}
          style={WORD_EDITOR_STYLE as CSSProperties}
          className="word-canvas size-full"
        />
      </WordEditorContext>
      <style href="word-canvas" precedence="default">
        {CANVAS_CSS}
      </style>
    </div>
  )
}

/** Shown instead of the editor when the file cannot be parsed. */
function OpenError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div
      role="alert"
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        padding: 24,
        textAlign: "center",
      }}
    >
      <span style={{ fontSize: 16, fontWeight: 600 }}>Can&apos;t open this document</span>
      <span style={{ fontSize: 13, color: "var(--of-ink-muted)", maxWidth: 480 }}>{message}</span>
      <button
        type="button"
        onClick={onRetry}
        style={{
          marginTop: 8,
          height: 32,
          padding: "0 16px",
          border: "none",
          borderRadius: 4,
          background: "var(--of-accent)",
          color: "#fff",
          font: "inherit",
          fontSize: 14,
          cursor: "pointer",
        }}
      >
        Retry
      </button>
    </div>
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
