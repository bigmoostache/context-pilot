import { Wrench } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { toolGroups } from "@/lib/mock"
import { PanelFrame } from "./PanelFrame"

/**
 * Tools panel maquette — the enabled tool registry, grouped by category. Each
 * row shows the tool name, a one-line description, and an enabled/disabled
 * status dot. Mirrors the real Tools panel that the agent consults to know its
 * own capabilities.
 */
export function ToolsPanel({ panel }: { panel: ContextPanel }) {
  const total = toolGroups.reduce((n, g) => n + g.tools.length, 0)
  const on = toolGroups.reduce((n, g) => n + g.tools.filter((t) => t.status === "on").length, 0)

  return (
    <PanelFrame
      icon={Wrench}
      name="Tools"
      subtitle={`${on} of ${total} enabled in view · 53 total`}
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <div className="flex flex-col gap-4">
        {toolGroups.map((g) => (
          <div key={g.category}>
            <div className="mb-1.5 text-[11px] font-medium uppercase tracking-wide text-muted-foreground/70">
              {g.category}
            </div>
            <ul className="overflow-hidden rounded-lg border border-border">
              {g.tools.map((t, i) => (
                <li
                  key={t.name}
                  className={`flex items-center gap-2.5 px-3 py-2 ${i % 2 ? "bg-muted/30" : "bg-card"}`}
                >
                  <span
                    className="size-1.5 shrink-0 rounded-full"
                    style={{ background: t.status === "on" ? "var(--ok)" : "var(--muted-foreground)" }}
                  />
                  <span className="shrink-0 font-mono text-[12px] text-foreground/90">{t.name}</span>
                  <span className="truncate text-[11.5px] text-muted-foreground/75">{t.desc}</span>
                  <span
                    className="ml-auto shrink-0 text-[10px] font-medium"
                    style={{ color: t.status === "on" ? "var(--ok)" : "var(--muted-foreground)" }}
                  >
                    {t.status === "on" ? "enabled" : "disabled"}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </PanelFrame>
  )
}
