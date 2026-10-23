import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react"
import type { FinderNode } from "@/lib/types"
import { saveOffice } from "@/lib/api"
import { useFsOffice, useFsOfficeVersion } from "@/lib/live"
import { Generic, PreviewStatus } from "../previewParts"
import { officeKindOf } from "../kinds"
import { usePptxFonts } from "./pptxFonts"
import "@betteroffice/docx-react/styles.css"

/** BetterOffice editors (Rust engines compiled to WASM) — loaded on first open. */
const LazyXlsx = lazy(() =>
  import("@betteroffice/xlsx-react").then((m) => ({ default: m.XlsxEditor })),
)
const LazyDocx = lazy(() =>
  import("@betteroffice/docx-react").then((m) => ({ default: m.DocxEditor })),
)
const LazyPptx = lazy(() =>
  import("@betteroffice/pptx-react").then((m) => ({ default: m.PptxEditor })),
)

type SaveState = "idle" | "saving" | "saved" | "conflict" | "error"

/**
 * Edit a realm .docx/.xlsx/.pptx in the BetterOffice editor. The file on disk is
 * the source of truth: the editor loads its bytes, saves the whole file back
 * (refused when an agent changed it meanwhile), and reloads by itself when the
 * polled version moves while nothing is unsaved.
 */
export function OfficeEditor({ fleetId, node }: { fleetId: string; node: FinderNode }) {
  // docx/pptx engines parse on the main thread (xlsx runs in a Web Worker but
  // still takes ~20 s on a 2 MB workbook). Never open on mount, so a restored
  // tab doesn't fetch and parse; open only after an explicit click.
  const [open, setOpen] = useState(false)
  if (!open) return <OpenGate node={node} onOpen={() => setOpen(true)} />
  return <OfficeEditorBody fleetId={fleetId} node={node} />
}

/** Click-to-open placeholder shown before the editor loads. */
function OpenGate({ node, onOpen }: { node: FinderNode; onOpen: () => void }) {
  const mb = node.size == null ? null : node.size / (1024 * 1024)
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-10 text-center">
      <span className="text-[12.5px] text-muted-foreground">
        {node.name}
        {mb == null ? "" : ` · ${mb.toFixed(1)} MB`}
      </span>
      <button
        type="button"
        onClick={onOpen}
        className="rounded-md bg-(--signal) px-3 py-1.5 text-[12px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105"
      >
        Open in editor
      </button>
      {/* xlsx parses in a Web Worker; docx/pptx still parse on the main thread. */}
      {mb != null && mb > 0.5 && officeKindOf(node.path) !== "xlsx" && (
        <span className="text-[11px] text-muted-foreground/70">
          Large file: the page may be unresponsive while it opens.
        </span>
      )}
    </div>
  )
}

/** The loaded editor: fetches the document and mounts the BetterOffice editor. */
function OfficeEditorBody({ fleetId, node }: { fleetId: string; node: FinderNode }) {
  const kind = officeKindOf(node.path)
  const doc = useFsOffice(fleetId, node.path)
  const polled = useFsOfficeVersion(fleetId, node.path)
  const baseRef = useRef("")
  const dirtyRef = useRef(false)
  const [state, setState] = useState<SaveState>("idle")
  // Version whose xlsx workbook finished opening (the engine signals onReady).
  const [xlsxReady, setXlsxReady] = useState("")
  const pptxFonts = usePptxFonts(kind === "pptx")
  const { refetch } = doc

  useEffect(() => {
    if (doc.data) baseRef.current = doc.data.version
  }, [doc.data])

  // An agent rewrote the file: reload when the user has nothing unsaved.
  const remote = polled.data?.version
  useEffect(() => {
    if (!remote || !baseRef.current || remote === baseRef.current || dirtyRef.current) return
    refetch()
  }, [remote, refetch])

  const save = useCallback(
    (bytes: Uint8Array) => {
      setState("saving")
      saveOffice(fleetId, node.path, bytes, baseRef.current)
        .then((res) => {
          baseRef.current = res.version
          dirtyRef.current = false
          setState("saved")
        })
        .catch((e: unknown) => {
          const msg = e instanceof Error ? e.message : ""
          setState(msg.includes("409") || msg.includes("changed on disk") ? "conflict" : "error")
        })
    },
    [fleetId, node.path],
  )
  const markDirty = useCallback(() => {
    dirtyRef.current = true
  }, [])

  if (!kind) return <Generic node={node} />
  if (doc.loading) return <PreviewStatus label="Loading document…" />
  if (doc.error || !doc.data) return <Generic node={node} />
  if (kind === "pptx" && pptxFonts.error) return <Generic node={node} />
  if (kind === "pptx" && !pptxFonts.fonts) return <PreviewStatus label="Loading fonts…" />

  const { bytes, version } = doc.data
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <SaveBar
        state={state}
        onReload={() => {
          dirtyRef.current = false
          setState("idle")
          refetch()
        }}
      />
      <div key={version} className="relative min-h-0 flex-1">
        <Suspense fallback={<PreviewStatus label="Loading editor…" />}>
          {kind === "xlsx" && (
            <>
              <LazyXlsx
                file={bytes}
                fileName={node.name}
                onSave={save}
                onChange={markDirty}
                onReady={() => setXlsxReady(version)}
                className="size-full"
              />
              {xlsxReady !== version && <OpeningOverlay />}
            </>
          )}
          {kind === "pptx" && (
            <LazyPptx
              file={bytes}
              fileName={node.name}
              fonts={pptxFonts.fonts ?? []}
              onSave={save}
              onChange={markDirty}
              className="size-full"
            />
          )}
          {kind === "docx" && (
            <LazyDocx
              documentBuffer={bytes}
              downloadOnSave={false}
              onSave={(buf: ArrayBuffer) => save(new Uint8Array(buf))}
            />
          )}
        </Suspense>
      </div>
    </div>
  )
}

/**
 * Covers the xlsx grid while the worker parses every sheet: a large workbook
 * (tens of MB unpacked) shows nothing for ~20 s otherwise.
 */
function OpeningOverlay() {
  return (
    <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-background">
      <span className="size-5 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-(--signal)" />
      <span className="text-[12.5px] text-muted-foreground">Opening workbook…</span>
      <span className="text-[11px] text-muted-foreground/70">
        Large workbooks can take up to ~20 s.
      </span>
    </div>
  )
}

/** One-line save status; a conflict offers to reload the agent's version. */
function SaveBar({ state, onReload }: { state: SaveState; onReload: () => void }) {
  if (state === "idle") return null
  const label: Record<Exclude<SaveState, "idle">, string> = {
    saving: "Saving…",
    saved: "Saved",
    conflict: "The file changed on disk since you opened it.",
    error: "Save failed.",
  }
  return (
    <div className="flex h-7 shrink-0 items-center gap-2 px-3 text-[11px] text-muted-foreground">
      <span
        className={state === "saved" ? "text-(--ok)" : state === "saving" ? "" : "text-(--danger)"}
      >
        {label[state]}
      </span>
      {state === "conflict" && (
        <button
          type="button"
          onClick={onReload}
          className="rounded-md bg-muted px-2 py-0.5 font-medium hover:bg-muted/70"
        >
          Reload (discard my edits)
        </button>
      )}
    </div>
  )
}
