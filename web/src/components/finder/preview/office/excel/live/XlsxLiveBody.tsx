import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react"
import type { XlsxCommandStore } from "@betteroffice/xlsx-react"
import type { FinderNode } from "@/lib/types"
import { Generic, PreviewStatus } from "../../../previewParts"
import { SaveBar, type SaveState } from "../../saveBar"
import { OpeningOverlay } from "./OpeningOverlay"
import { useXlsxLive } from "./useXlsxLive"

const LazyExcel = lazy(() => import("../ExcelXlsx"))

/** Quiet time after the last edit before autosave writes the workbook. */
const AUTOSAVE_MS = 1200

/** The part of any editor variant's onReady api autosave drives. */
interface EditorApi {
  readonly commands: XlsxCommandStore
}

/**
 * The xlsx editor on a live session: edits autosave through the office log
 * (no Ctrl+S, no remount) and agent edits merge in without a reload.
 */
export function XlsxLiveBody({ fleetId, node }: { fleetId: string; node: FinderNode }) {
  const live = useXlsxLive(fleetId, node.path)
  const [state, setState] = useState<SaveState>("idle")
  const [ready, setReady] = useState(0)
  const autosave = useAutosave(live.save, setState)
  const key = live.session?.key ?? 0

  const onChange = useCallback(() => {
    live.markDirty()
    autosave.schedule()
  }, [live, autosave])

  if (live.error) return <Generic node={node} />
  if (!live.session || !live.collaboration) return <PreviewStatus label="Loading document…" />

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Autosave is silent: only a failure or a disk conflict takes space. */}
      <SaveBar
        state={state === "saving" || state === "saved" ? "idle" : state}
        onReload={() => {
          setState("idle")
          live.reopen()
        }}
      />
      <div key={key} className="relative min-h-0 flex-1">
        <Suspense fallback={<PreviewStatus label="Loading editor…" />}>
          <LazyExcel
            bytes={live.session.base}
            fileName={node.name}
            collaboration={live.collaboration}
            onSave={autosave.write}
            onChange={onChange}
            onReady={(api) => {
              autosave.attach(api)
              setReady(key)
            }}
          />
        </Suspense>
        {ready !== key && <OpeningOverlay size={live.session.base.length} />}
      </div>
    </div>
  )
}

/**
 * Debounced, serialized saves: after {@link AUTOSAVE_MS} of quiet the editor's
 * `save` command produces the bytes (its `onSave` lands in `write`), and one
 * post runs at a time; edits made during a post schedule another round.
 */
function useAutosave(post: (bytes: Uint8Array) => Promise<void>, setState: (s: SaveState) => void) {
  const apiRef = useRef<EditorApi | null>(null)
  const timerRef = useRef<number | undefined>(undefined)
  const busyRef = useRef(false)
  const againRef = useRef(false)

  const run = useCallback(() => {
    const api = apiRef.current
    if (!api) return
    if (busyRef.current) {
      againRef.current = true
      return
    }
    busyRef.current = true
    api.commands.execute("save", null).catch(() => {
      busyRef.current = false
      setState("error")
    })
  }, [setState])

  const schedule = useCallback(() => {
    window.clearTimeout(timerRef.current)
    timerRef.current = window.setTimeout(run, AUTOSAVE_MS)
  }, [run])

  const write = useCallback(
    (bytes: Uint8Array) => {
      busyRef.current = true
      setState("saving")
      post(bytes)
        .then(() => {
          setState("saved")
        })
        .catch((e: unknown) => {
          const msg = e instanceof Error ? e.message : ""
          setState(msg.includes("409") || msg.includes("changed on disk") ? "conflict" : "error")
        })
        .finally(() => {
          busyRef.current = false
          if (againRef.current) {
            againRef.current = false
            schedule()
          }
        })
    },
    [post, setState, schedule],
  )

  const attach = useCallback((api: EditorApi) => {
    apiRef.current = api
  }, [])

  useEffect(
    () => () => {
      window.clearTimeout(timerRef.current)
    },
    [],
  )

  return { schedule, write, attach }
}
