import { Radar } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { useRadar } from "@/lib/live"
import { PanelFrame, PanelSection, ImportanceDot, InspectionUnavailable } from "./PanelFrame"

/**
 * Context Radar panel — recency-weighted recall surfaced from the
 * logs. Anchor signals (the most recent task contexts) sit at the top; below,
 * scored results are ranked by relevance with a horizontal score meter, an
 * importance dot, and their timestamp.
 */
export function RadarPanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: radarData } = useRadar(agentId)
  const radarAnchors = radarData?.anchors ?? []
  const radarResults = radarData?.results ?? []
  return (
    <PanelFrame
      icon={Radar}
      name="Context Radar"
      subtitle="Recency-weighted recall · half-life 100 logs"
      tokens={panel.tokens}
      cost={panel.costUsd}
      accent="var(--interactive)"
    >
      {radarAnchors.length === 0 && radarResults.length === 0 ? (
        <InspectionUnavailable reason="The Context Radar is a live half-life ranking the running agent computes over its logs; it isn't persisted as a consumable artifact. Surfacing it requires the agent to publish its radar — a tracked follow-up." />
      ) : (
        <>
      <PanelSection label="Anchors · live signals">
        <ul className="flex flex-col gap-1.5">
          {radarAnchors.map((a, i) => (
            <li key={i} className="flex gap-2.5 rounded-md bg-muted/40 px-2.5 py-1.5">
              <span className="shrink-0 font-mono text-[10px] tabular-nums text-[var(--signal)]">
                {a.time}
              </span>
              <span className="text-[11.5px] leading-snug text-foreground/80">{a.signal}</span>
            </li>
          ))}
        </ul>
      </PanelSection>

      <PanelSection label={`Results · ${radarResults.length} scored`}>
        <ul className="flex flex-col gap-2">
          {radarResults.map((r, i) => (
            <li key={i} className="rounded-md border border-border/70 px-2.5 py-2">
              <div className="mb-1 flex items-center gap-2">
                <ImportanceDot level={r.importance} />
                <span className="font-mono text-[10px] tabular-nums text-muted-foreground/70">
                  {r.datetime}
                </span>
                <span className="ml-auto font-mono text-[10px] tabular-nums text-foreground/70">
                  {r.score.toFixed(3)}
                </span>
              </div>
              <div className="mb-1.5 h-1 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full fill-sweep"
                  style={{ width: `${r.score * 100}%`, background: "var(--interactive)" }}
                />
              </div>
              <p className="text-[11.5px] leading-snug text-foreground/80">{r.content}</p>
            </li>
          ))}
        </ul>
      </PanelSection>
        </>
      )}
    </PanelFrame>
  )
}
