import { NotebookPen } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { useScratchpad } from "@/lib/live"
import { PanelFrame } from "../../PanelFrame"

/**
 * Scratchpad panel — ephemeral per-worker working cells (notes, plans,
 * snippets). Each cell shows its id, title, and a content preview.
 */
export function ScratchpadPanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: scratchCells = [] } = useScratchpad(agentId)
  return (
    <PanelFrame
      icon={NotebookPen}
      name="Scratchpad"
      subtitle={`${scratchCells.length} cells`}
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <div className="flex flex-col gap-2.5">
        {scratchCells.map((c) => (
          <article key={c.id} className="card-shadow rounded-lg border border-border bg-card p-3">
            <div className="mb-1 flex items-center gap-2">
              <span className="font-mono text-[10px] text-muted-foreground/60">{c.id}</span>
              <span className="text-[12.5px] font-medium text-foreground/90">{c.title}</span>
            </div>
            <p className="text-[11.5px] leading-snug text-muted-foreground/80">{c.preview}</p>
          </article>
        ))}
        {scratchCells.length === 0 && (
          <div className="rounded-lg border border-dashed border-border py-8 text-center text-[12px] text-muted-foreground/60">
            Scratchpad empty
          </div>
        )}
      </div>
    </PanelFrame>
  )
}
