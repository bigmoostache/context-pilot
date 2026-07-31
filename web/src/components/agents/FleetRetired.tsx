import { ArchiveRestore, Bot, FolderGit2, Loader2 } from "lucide-react"
import { useRetiredFleet, useUnretireAgent } from "@/lib/live"
import type { Agent } from "@/lib/types"
import { cn } from "@/lib/utils"

/** Retired-row grid: name+folder grows, then model · unretire action. Its own
 *  (shorter) template — a retired agent has no live status/health/cost. */
const RETIRED_GRID = "grid grid-cols-[minmax(0,1fr)_170px_112px] items-center gap-3"

/**
 * The Retired (archived) agents section — rendered below the active fleet only
 * when at least one agent is retired (T271). A muted list mirroring the active
 * fleet's Linear-style rows: identity + kept realm + a one-click Unretire that
 * respawns the agent on its folder. Retired agents have no live process, so
 * there is no status / health / cost — just identity + the restore affordance.
 *
 * Extracted from FleetDashboard (T637) so the dashboard stays under the 500-line
 * cap; the mobile twin (`mobile-components/agents/FleetRetired`) mirrors it.
 */
export function RetiredSection({ onFlash }: { onFlash: (m: string) => void }) {
  const { data: retired } = useRetiredFleet()
  if (!retired || retired.length === 0) return null

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <h2 className="text-[12px] font-semibold tracking-[0.06em] text-muted-foreground/80 uppercase">
          Retired
        </h2>
        <span className="rounded-full bg-muted/60 px-1.5 py-0.5 text-[10.5px] font-medium text-muted-foreground/70">
          {retired.length}
        </span>
      </div>
      <div className="overflow-hidden rounded-xl border border-dashed border-border bg-card/40">
        <div className="divide-y divide-border/50">
          {retired.map((a) => (
            <RetiredRow key={a.id} agent={a} onFlash={onFlash} />
          ))}
        </div>
      </div>
    </section>
  )
}

function RetiredRow({ agent, onFlash }: { agent: Agent; onFlash: (m: string) => void }) {
  const unretire = useUnretireAgent()

  const onUnretire = () => {
    if (unretire.isPending) return
    unretire.mutate(agent.id, {
      onSuccess: () => onFlash(`Bringing ${agent.name} back — it will reconnect in a moment`),
      onError: (e) => onFlash(e instanceof Error ? e.message : `Could not unretire ${agent.name}`),
    })
  }

  return (
    <div className={cn(RETIRED_GRID, "px-4 py-2.5")}>
      {/* Identity — icon + name over the kept realm path. */}
      <div className="flex min-w-0 items-center gap-3">
        <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted/50 text-muted-foreground">
          <FolderGit2 className="size-4" />
        </span>
        <div className="flex min-w-0 flex-col leading-tight">
          <span className="truncate text-[13.5px] font-semibold text-foreground/70">
            {agent.name}
          </span>
          <span className="truncate text-[11px] text-muted-foreground/60">{agent.folder}</span>
        </div>
      </div>

      {/* Model */}
      <span className="inline-flex min-w-0 items-center gap-1.5 text-[11.5px] text-muted-foreground">
        <Bot className="size-3.5 shrink-0" />
        <span className="truncate">{agent.model}</span>
      </span>

      {/* Unretire */}
      <button
        onClick={onUnretire}
        disabled={unretire.isPending}
        className="flex items-center justify-center gap-1.5 rounded-lg border border-border bg-muted/40 px-3 py-1.5 text-[12px] font-medium text-foreground/70 transition-colors hover:border-(--interactive)/50 hover:text-(--interactive) disabled:cursor-not-allowed disabled:opacity-50"
      >
        {unretire.isPending ? (
          <Loader2 className="size-3.5 animate-spin" />
        ) : (
          <ArchiveRestore className="size-3.5" />
        )}
        Unretire
      </button>
    </div>
  )
}
