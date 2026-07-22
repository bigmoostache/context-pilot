import { ArchiveRestore, Bot, FolderGit2, Loader2 } from "lucide-react"
import { useRetiredFleet, useUnretireAgent } from "@/lib/live"
import type { Agent } from "@/lib/types"

/**
 * The Retired (archived) agents section — rendered below the active fleet only
 * when at least one agent is retired (T271). Each card shows the kept realm and
 * a one-click Unretire that respawns the agent on its folder. Retired agents
 * have no live process, so there is no status pill / health badge / cost — just
 * identity + the restore affordance.
 *
 * Extracted from FleetDashboard (T637) so the dashboard stays under the 500-line
 * cap; the mobile twin (`mobile-components/agents/FleetRetired`) mirrors it.
 */
export function RetiredSection({ onFlash }: { onFlash: (m: string) => void }) {
  const { data: retired } = useRetiredFleet()
  if (!retired || retired.length === 0) return null

  return (
    <section className="flex flex-col gap-3.5">
      <div className="flex items-center gap-2">
        <h2 className="text-[13px] font-semibold tracking-[0.06em] text-muted-foreground/80 uppercase">
          Retired
        </h2>
        <span className="rounded-full bg-muted/60 px-1.5 py-0.5 text-[10.5px] font-medium text-muted-foreground/70">
          {retired.length}
        </span>
      </div>
      <div className="grid grid-cols-1 gap-3.5 md:grid-cols-2">
        {retired.map((a) => (
          <RetiredCard key={a.id} agent={a} onFlash={onFlash} />
        ))}
      </div>
    </section>
  )
}

function RetiredCard({ agent, onFlash }: { agent: Agent; onFlash: (m: string) => void }) {
  const unretire = useUnretireAgent()

  const onUnretire = () => {
    if (unretire.isPending) return
    unretire.mutate(agent.id, {
      onSuccess: () => onFlash(`Bringing ${agent.name} back — it will reconnect in a moment`),
      onError: (e) => onFlash(e instanceof Error ? e.message : `Could not unretire ${agent.name}`),
    })
  }

  return (
    <div className="flex flex-col gap-3 rounded-xl border border-dashed border-border bg-card/50 p-4 transition-colors hover:border-(--interactive)/45">
      <div className="flex items-center gap-3">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted/50 text-muted-foreground">
          <FolderGit2 className="size-5" />
        </span>
        <div className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className="truncate text-[14px] font-semibold text-foreground/75">
            {agent.name}
          </span>
          <span className="truncate text-[11px] text-muted-foreground/60">{agent.folder}</span>
        </div>
        <span className="inline-flex shrink-0 items-center rounded-full bg-muted/60 px-2 py-0.5 text-[10.5px] font-medium text-muted-foreground/70">
          Retired
        </span>
      </div>

      <div className="flex items-center gap-4 text-[11px] text-muted-foreground">
        <span className="inline-flex items-center gap-1">
          <Bot className="size-3.5" />
          {agent.model}
        </span>
      </div>

      <button
        onClick={onUnretire}
        disabled={unretire.isPending}
        className="mt-0.5 flex items-center justify-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2 text-[12.5px] font-medium text-foreground/70 transition-colors hover:border-(--interactive)/50 hover:text-(--interactive) disabled:cursor-not-allowed disabled:opacity-50"
      >
        {unretire.isPending ? (
          <Loader2 className="size-4 animate-spin" />
        ) : (
          <ArchiveRestore className="size-4" />
        )}
        Unretire
      </button>
    </div>
  )
}
