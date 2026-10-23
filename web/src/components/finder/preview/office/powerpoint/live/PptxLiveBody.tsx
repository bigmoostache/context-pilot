import { lazy, Suspense, useState } from "react"
import type { FinderNode } from "@/lib/types"
import { PreviewStatus } from "../../../previewParts"
import { PptChrome } from "../PptChrome"
import { PptRibbon } from "../PptRibbon"
import { usePptxFonts } from "./pptxFonts"
import { usePptxLive } from "./pptxLive"
import { SaveBar, type SaveState } from "../../saveBar"

const LazyPptx = lazy(() =>
  import("@betteroffice/pptx-react").then((m) => ({ default: m.PptxEditor })),
)

/**
 * The pptx editor on a live session: agent edits merge in without a reload.
 * Retry remounts the whole session (log fetch, fonts, editor) from scratch.
 */
export function PptxLiveBody({ fleetId, node }: { fleetId: string; node: FinderNode }) {
  const [attempt, setAttempt] = useState(0)
  return (
    <LiveSession
      key={attempt}
      fleetId={fleetId}
      node={node}
      onRetry={() => setAttempt(attempt + 1)}
    />
  )
}

function LiveSession({
  fleetId,
  node,
  onRetry,
}: {
  fleetId: string
  node: FinderNode
  onRetry: () => void
}) {
  const live = usePptxLive(fleetId, node.path)
  const fonts = usePptxFonts(true)
  const [state, setState] = useState<SaveState>("idle")
  const [engineError, setEngineError] = useState<string | null>(null)

  const message = openErrorOf(live.error, fonts.error, live.session?.base, engineError)
  if (message !== null) {
    return (
      <PptChrome fileName={node.name}>
        <OpenError message={message} onRetry={onRetry} />
      </PptChrome>
    )
  }
  if (!live.session || !live.collaboration) return <PreviewStatus label="Loading document…" />
  if (!fonts.fonts) return <PreviewStatus label="Loading fonts…" />

  const save = (bytes: Uint8Array) => {
    setState("saving")
    live
      .save(bytes)
      .then(() => setState("saved"))
      .catch((e: unknown) => {
        const msg = e instanceof Error ? e.message : ""
        setState(msg.includes("409") || msg.includes("changed on disk") ? "conflict" : "error")
      })
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <SaveBar
        state={state}
        onReload={() => {
          setState("idle")
          live.reopen()
        }}
      />
      <div key={live.session.key} className="relative min-h-0 flex-1">
        <PptChrome fileName={node.name}>
          <Suspense fallback={<PreviewStatus label="Loading editor…" />}>
            <LazyPptx
              file={live.session.base}
              fileName={node.name}
              fonts={fonts.fonts}
              collaboration={live.collaboration}
              onSave={save}
              onChange={live.markDirty}
              onError={(e: Error) =>
                setEngineError(e.message || "The presentation could not be read.")
              }
              toolbar={<PptRibbon />}
              className="size-full"
            />
          </Suspense>
        </PptChrome>
      </div>
    </div>
  )
}

/** Why the deck can't open, or `null` while it can (or is still loading). */
function openErrorOf(
  liveError: boolean,
  fontsError: unknown,
  base: Uint8Array | undefined,
  engineError: string | null,
): string | null {
  if (liveError) return "The file could not be loaded from the server."
  if (base && !isZip(base)) return "The file is not a PowerPoint presentation (no .pptx container)."
  if (engineError !== null) return engineError
  if (fontsError) return "The presentation fonts could not be loaded."
  return null
}

/** A .pptx is a zip: it starts with the local-file-header magic "PK\x03\x04". */
const ZIP_MAGIC = [80, 75, 3, 4]

function isZip(bytes: Uint8Array) {
  return ZIP_MAGIC.every((b, i) => bytes[i] === b)
}

/** Shown inside the PowerPoint frame instead of the editor when the deck can't open. */
function OpenError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div
      role="alert"
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        padding: 24,
        textAlign: "center",
      }}
    >
      <span style={{ fontSize: 16, fontWeight: 600 }}>Can&apos;t open this presentation</span>
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
