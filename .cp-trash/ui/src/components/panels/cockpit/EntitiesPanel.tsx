import { Database } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { entityTables } from "@/lib/mock"
import { PanelFrame } from "./PanelFrame"

/**
 * Entities panel maquette — the structured SQLite domain database. One card per
 * table shows its name, row count, a compact column signature (PK first), and a
 * couple of sample rows in mono. Mirrors the real Entities panel the agent uses
 * for queryable structured knowledge.
 */
export function EntitiesPanel({ panel }: { panel: ContextPanel }) {
  const totalRows = entityTables.reduce((n, t) => n + t.rows, 0)

  return (
    <PanelFrame
      icon={Database}
      name="Entities"
      subtitle={`${entityTables.length} tables · ${totalRows.toLocaleString()} rows`}
      tokens={panel.tokens}
      cost={panel.costUsd}
      accent="var(--interactive)"
    >
      <div className="flex flex-col gap-3">
        {entityTables.map((t) => (
          <article key={t.name} className="overflow-hidden rounded-lg border border-border card-shadow">
            <header className="flex items-center gap-2 border-b border-border bg-card px-3 py-2">
              <Database className="size-3.5 text-[var(--interactive)]" />
              <span className="font-mono text-[12.5px] font-semibold text-foreground/90">{t.name}</span>
              <span className="ml-auto rounded-md bg-muted/70 px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground">
                {t.rows.toLocaleString()} rows
              </span>
            </header>
            <div className="bg-background px-3 py-2">
              <div className="mb-2 font-mono text-[10.5px] leading-relaxed text-muted-foreground/80">
                {t.columns}
              </div>
              <ul className="flex flex-col gap-1">
                {t.samples.map((s, i) => (
                  <li key={i} className="truncate font-mono text-[10.5px] text-foreground/70">
                    {s}
                  </li>
                ))}
              </ul>
            </div>
          </article>
        ))}
      </div>
    </PanelFrame>
  )
}
