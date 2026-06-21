import { Brain } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { useMemory } from "@/lib/live"
import { PanelFrame, ImportanceDot, Chip } from "../../PanelFrame"

/**
 * Memories panel — long-term recall cards. Each card leads with its
 * id and an importance dot, states its tl;dr, and tags itself with freeform
 * labels. Mirrors the real Memories panel which persists across the whole
 * conversation in memories.yaml.
 */
export function MemoryPanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: memoryCards = [] } = useMemory(agentId)
  return (
    <PanelFrame
      icon={Brain}
      name="Memories"
      subtitle={`${memoryCards.length} of 52 · long-term recall`}
      tokens={panel.tokens}
      cost={panel.costUsd}
      accent="var(--interactive)"
    >
      <div className="flex flex-col gap-2.5">
        {memoryCards.map((m) => (
          <article
            key={m.id}
            className="rounded-lg border border-border bg-card p-3 card-shadow"
          >
            <div className="mb-1.5 flex items-center gap-2">
              <ImportanceDot level={m.importance} />
              <span className="font-mono text-[11px] font-semibold text-foreground/80">{m.id}</span>
              <span className="text-[10px] uppercase tracking-wide text-muted-foreground/60">
                {m.importance}
              </span>
            </div>
            <p className="text-[12.5px] leading-snug text-foreground/85">{m.tldr}</p>
            <div className="mt-2 flex flex-wrap gap-1.5">
              {m.labels.map((l) => (
                <Chip key={l}>{l}</Chip>
              ))}
            </div>
          </article>
        ))}
      </div>
    </PanelFrame>
  )
}
