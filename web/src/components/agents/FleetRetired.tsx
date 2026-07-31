import { ArchiveRestore, Bot, FolderGit2, Loader2 } from "lucide-react"
import { useRetiredFleet, useUnretireAgent } from "@/lib/live"
import type { Agent } from "@/lib/types"

/**
 * The Retired (archived) agents section — rendered below the active fleet only
 * when at least one agent is retired (T271). A muted run of soft hover-card rows
 * matching the active groups: identity + kept realm + a hover Unretire that
 * respawns the agent on its folder. No live process, so no status / cost.
 *
 * Extracted from FleetDashboard (T637) so the dashboard stays under the 500-line
 * cap; the mobile twin (`mobile-components/agents/FleetRetired`) mirrors it.
 */
export function RetiredSection({ onFlash }: { onFlash: (m: string) => void }) {
  const { data: retired } = useRetiredFleet()
  if (!retired || retired.length === 0) return null

  return (
    <section className="mt-6">
      <div className="flex items-center gap-2 px-2.5 pt-3 pb-1.5">
        <span className="text-[11px] font-semibold text-muted-foreground">Retired</span>
        <span className="text-[11px] text-muted-foreground/45 tabular-nums">{retired.length}</span>
      </div>
      {retired.map((a) => (
        <RetiredRow key={a.id} agent={a} onFlash={onFlash} />
      ))}
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
    <div className="group hover:card-shadow mb-0.5 flex items-center gap-3 rounded-lg px-2.5 py-2 transition-colors hover:bg-card">
      {/* Identity — muted folder chip + name + the kept realm path trailing. */}
      <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-muted/50 text-muted-foreground/70">
        <FolderGit2 className="size-3.5" />
      </span>
      <span className="shrink-0 truncate text-[13px] font-medium text-foreground/60">
        {agent.name}
      </span>
      <span className="min-w-0 flex-1 truncate text-[11.5px] text-muted-foreground/45">
        {agent.folder}
      </span>

      {/* Model */}
      <span className="hidden shrink-0 items-center gap-1.5 text-[11.5px] text-muted-foreground/60 sm:inline-flex">
        <Bot className="size-3.5 text-muted-foreground/40" />
        {agent.model}
      </span>

      {/* Unretire — revealed on row hover. */}
      <button
        onClick={onUnretire}
        disabled={unretire.isPending}
        className="flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1 text-[12px] text-muted-foreground/60 opacity-0 transition-[opacity,color,background-color] group-hover:opacity-100 hover:bg-muted hover:text-(--interactive) disabled:opacity-50 disabled:hover:bg-transparent"
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
