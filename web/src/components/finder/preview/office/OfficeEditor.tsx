import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react"
import type { FinderNode } from "@/lib/types"
import { saveOffice } from "@/lib/api"
import { useFsOffice, useFsOfficeVersion } from "@/lib/live"
import { Generic, PreviewStatus } from "../previewParts"
import { officeKindOf } from "../kinds"
import { XlsxLiveBody } from "./excel/live/XlsxLiveBody"
import { PptxLiveBody } from "./powerpoint/live/PptxLiveBody"
import { SaveBar, type SaveState } from "./saveBar"
import "@betteroffice/docx-react/styles.css"

/** BetterOffice docx editor (Rust engine compiled to WASM) — loaded on first open. */
const LazyDocx = lazy(() => import("./word/WordDocx"))

/**
 * Edit a realm .docx/.xlsx/.pptx in the BetterOffice editor. xlsx and pptx run
 * on a live office-log session (autosave / in-place agent edits); docx saves
 * the whole file back (refused when an agent changed it meanwhile) and reloads
 * by itself when the polled version moves while nothing is unsaved.
 */
export function OfficeEditor({ fleetId, node }: { fleetId: string; node: FinderNode }) {
  // docx/pptx engines parse on the main thread (xlsx runs in a Web Worker but
  // still takes seconds on a 2 MB workbook). Never open on mount, so a restored
  // tab doesn't fetch and parse; open only after an explicit click.
  const [open, setOpen] = useState(false)
  if (!open) return <OpenGate node={node} onOpen={() => setOpen(true)} />
  const kind = officeKindOf(node.path)
  if (kind === "pptx") return <PptxLiveBody fleetId={fleetId} node={node} />
  if (kind === "xlsx") return <XlsxLiveBody fleetId={fleetId} node={node} />
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

/** The loaded docx editor: fetches the document and mounts the BetterOffice editor. */
function OfficeEditorBody({ fleetId, node }: { fleetId: string; node: FinderNode }) {
  const kind = officeKindOf(node.path)
  const doc = useFsOffice(fleetId, node.path)
  const polled = useFsOfficeVersion(fleetId, node.path)
  const baseRef = useRef("")
  const dirtyRef = useRef(false)
  const [state, setState] = useState<SaveState>("idle")
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

  if (kind !== "docx") return <Generic node={node} />
  if (doc.loading) return <PreviewStatus label="Loading document…" />
  if (doc.error || !doc.data) return <Generic node={node} />

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
          <LazyDocx bytes={bytes} fileName={node.name} onSave={save} onRetry={refetch} />
        </Suspense>
      </div>
    </div>
  )
}
