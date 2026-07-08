import { Webhook } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { useCallbacks } from "@/lib/live"
import { PanelFrame } from "../../PanelFrame"

/**
 * Callbacks panel — file-edit hooks that auto-fire on matching globs.
 * Each row shows the id, name, the glob pattern, whether it blocks the edit
 * result, its timeout, scope, and working directory.
 */
export function CallbacksPanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: callbackRows = [] } = useCallbacks(agentId)
  return (
    <PanelFrame
      icon={Webhook}
      name="Callbacks"
      subtitle={`${callbackRows.length} edit hooks`}
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <ul className="flex flex-col gap-2">
        {callbackRows.map((c) => (
          <li key={c.id} className="card-shadow rounded-lg border border-border bg-card p-3">
            <div className="mb-1.5 flex items-center gap-2">
              <span className="font-mono text-[11px] font-semibold text-(--signal)">{c.id}</span>
              <span className="text-[12.5px] font-medium text-foreground/90">{c.name}</span>
              {c.blocking && (
                <span className="ml-auto rounded-md bg-(--warn)/15 px-1.5 py-0.5 text-[10px] font-medium text-(--warn)">
                  blocking
                </span>
              )}
            </div>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground/75">
              <span>
                pattern <code className="font-mono text-foreground/80">{c.pattern}</code>
              </span>
              <span>timeout {c.timeout}</span>
              <span>scope {c.scope}</span>
              <span>cwd {c.cwd}</span>
            </div>
          </li>
        ))}
      </ul>
    </PanelFrame>
  )
}
