import { useState } from "react"
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
function isZip(bytes: Uint8Array) {
  return bytes[0] === 0x50 && bytes[1] === 0x4b && bytes[2] === 0x03 && bytes[3] === 0x04
}

function EditorArea({
  bytes,
  onSave,
  onError,
}: {
  bytes: Uint8Array
  onSave: (bytes: Uint8Array) => void
  onError: (message: string) => void
}) {
  return (
    <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
      <DocxEditor
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
        className="size-full"
      />
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
