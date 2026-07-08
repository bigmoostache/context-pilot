import { Activity } from "lucide-react"
import type { ContextPanel, NotifKind } from "@/lib/types"
import { useSpine } from "@/lib/live"
import { PanelFrame, PanelSection } from "../../PanelFrame"

const KIND_COLOR: Record<NotifKind, string> = {
  user: "var(--signal)",
  reload: "var(--interactive)",
  custom: "var(--muted-foreground)",
}

/**
 * Spine panel — the auto-continuation nerve. Lists notifications
 * (user input, reload-resume, custom nudges) with a kind dot, id, time, and
 * processed state, plus the live guard-rail / auto-continuation config.
 */
export function SpinePanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: spine = [] } = useSpine(agentId)
  return (
    <PanelFrame
      icon={Activity}
      name="Spine"
      subtitle="Auto-continuation · notifications · guard rails"
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <PanelSection label="Recent notifications">
        <ul className="flex flex-col gap-1.5">
          {spine.map((n) => (
            <li key={n.id} className="flex gap-2.5 rounded-md border border-border/70 px-2.5 py-2">
              <span
                className="mt-1 size-2 shrink-0 rounded-full"
                style={{ background: KIND_COLOR[n.kind] }}
              />
              <div className="flex min-w-0 flex-1 flex-col">
                <div className="flex items-center gap-2 text-[10px] text-muted-foreground/65">
                  <span className="font-mono">{n.id}</span>
                  <span>· {n.time}</span>
                  <span>· {n.kind}</span>
                  {n.processed && <span className="ml-auto text-(--ok)">processed ✓</span>}
                </div>
                <p className="mt-0.5 text-[11.5px] leading-snug text-foreground/80">{n.text}</p>
              </div>
            </li>
          ))}
        </ul>
      </PanelSection>

      <PanelSection label="Config">
        <div className="card-shadow rounded-lg border border-border bg-card p-3 text-[11.5px]">
          <Row k="continue_until_todos_done" v="false" />
          <Row k="auto_continuation_count" v="6" />
          <Row k="max_auto_retries" v="40" />
          <Row k="autonomous mode" v="manual sailing" accent="var(--signal)" />
        </div>
      </PanelSection>
    </PanelFrame>
  )
}

function Row({ k, v, accent }: { k: string; v: string; accent?: string }) {
  return (
    <div className="flex items-center justify-between border-b border-border/50 py-1 last:border-0">
      <span className="font-mono text-[11px] text-muted-foreground/80">{k}</span>
      <span
        className="font-mono text-[11px] tabular-nums"
        style={{ color: accent ?? "var(--foreground)" }}
      >
        {v}
      </span>
    </div>
  )
}
