import { lazy, Suspense, useState } from "react"
import type { FinderNode } from "@/lib/types"
import { Generic, PreviewStatus } from "../previewParts"
import { PptChrome } from "./powerpoint/PptChrome"
import { PptRibbon } from "./powerpoint/PptRibbon"
import { usePptxFonts } from "./pptxFonts"
import { usePptxLive } from "./pptxLive"
import { SaveBar, type SaveState } from "./saveBar"

const LazyPptx = lazy(() =>
  import("@betteroffice/pptx-react").then((m) => ({ default: m.PptxEditor })),
)

/** The pptx editor on a live session: agent edits merge in without a reload. */
export function PptxLiveBody({ fleetId, node }: { fleetId: string; node: FinderNode }) {
  const live = usePptxLive(fleetId, node.path)
  const fonts = usePptxFonts(true)
  const [state, setState] = useState<SaveState>("idle")

  if (live.error || fonts.error) return <Generic node={node} />
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
              toolbar={<PptRibbon />}
              className="size-full"
            />
          </Suspense>
        </PptChrome>
      </div>
    </div>
  )
}
