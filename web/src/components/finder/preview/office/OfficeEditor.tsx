import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react"
import type { FinderNode } from "@/lib/types"
import { saveOffice } from "@/lib/api"
import { useFsOffice, useFsOfficeVersion } from "@/lib/live"
import { Generic, PreviewStatus } from "../previewParts"
import { officeKindOf } from "../kinds"
import { PptxLiveBody } from "./powerpoint/live/PptxLiveBody"
import { SaveBar, type SaveState } from "./saveBar"
import "@betteroffice/docx-react/styles.css"

/** BetterOffice editors (Rust engines compiled to WASM) — loaded on first open. */
const LazyXlsx = lazy(() => import("./excel/ExcelXlsx"))
const LazyDocx = lazy(() => import("./word/WordDocx"))

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
  if (officeKindOf(node.path) === "pptx") return <PptxLiveBody fleetId={fleetId} node={node} />
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
                bytes={bytes}
                fileName={node.name}
                onSave={save}
                onChange={markDirty}
                onReady={() => {
                  setXlsxReady(version)
                }}
              />
              {xlsxReady !== version && <OpeningOverlay size={bytes.length} />}
            </>
          )}
          {kind === "docx" && <LazyDocx bytes={bytes} fileName={node.name} onSave={save} />}
        </Suspense>
      </div>
    </div>
  )
}

/**
 * Covers the xlsx grid while the worker parses every sheet: a large workbook
 * (tens of MB unpacked) shows nothing for ~20 s otherwise.
 */
function OpeningOverlay({ size }: { size: number }) {
  const pct = useEstimatedProgress(size)
  return (
    <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-background">
      <span className="size-5 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-(--signal)" />
      <span className="text-[12.5px] text-muted-foreground">Opening workbook…</span>
      <div className="h-1 w-48 overflow-hidden rounded-full bg-muted">
        <div className="h-full bg-(--signal)" style={{ width: `${pct.toFixed(1)}%` }} />
      </div>
      <span className="text-[11px] text-muted-foreground/70">
        Large workbooks can take up to ~20 s.
      </span>
    </div>
  )
}

/** Measured worker open rate: roughly 8 s per compressed MB (1.2 MB 6.7 s, 1.8 MB 20 s). */
const OPEN_MS_PER_MB = 8000

/**
 * Estimated open progress (0–95). The engine reports none, so the bar follows
 * an exponential curve reaching ~85% at the size-based estimate, capped at 95%.
 */
function useEstimatedProgress(size: number): number {
  const [pct, setPct] = useState(0)
  useEffect(() => {
    const estimate = Math.max(1000, (size / (1024 * 1024)) * OPEN_MS_PER_MB)
    const start = performance.now()
    const id = window.setInterval(() => {
      const t = performance.now() - start
      setPct(95 * (1 - Math.exp((-2.3 * t) / estimate)))
    }, 200)
    return () => {
      window.clearInterval(id)
    }
  }, [size])
  return pct
}
