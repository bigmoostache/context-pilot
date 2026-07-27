import { ArchiveRestore, FolderGit2, Loader2 } from "lucide-react"
import { useRetiredFleet, useUnretireAgent } from "@/lib/live"
import type { Agent } from "@/lib/types"
import { useSwipeRow } from "@/lib/live/useSwipeRow"

/**
 * Retired (archived) agents — mobile twin of `components/agents/FleetRetired`.
 * Same item styling as the live fleet rows, muted, shown only when at least one
 * agent is retired (T271). A retired agent has no live process to open, so the
 * row isn't tap-to-open; a swipe-left reveals its single Unretire action.
 *
 * Extracted from the mobile FleetDashboard (T637) so the dashboard stays under
 * the 500-line cap.
 */
export function RetiredSection({ onFlash }: { onFlash: (m: string) => void }) {
  const { data: retired } = useRetiredFleet()
  if (!retired || retired.length === 0) return null

  return (
    <section className="mt-4 flex flex-col">
      <div className="flex items-center gap-2 px-4 pb-1">
        <h2 className="text-[12.5px] font-semibold tracking-[0.06em] text-muted-foreground/80 uppercase">
          Retired
        </h2>
        <span className="rounded-full bg-muted/60 px-1.5 py-0.5 text-[10.5px] font-medium text-muted-foreground/70">
          {retired.length}
        </span>
      </div>
      <ul className="flex flex-col">
        {retired.map((a) => (
          <li key={a.id}>
            <RetiredSwipeRow agent={a} onFlash={onFlash} />
          </li>
        ))}
      </ul>
    </section>
  )
}

/** A retired agent row — swipe-left reveals a single Unretire action (respawns
 *  it on its kept folder). No tap-to-open (there's no live process). */
function RetiredSwipeRow({ agent, onFlash }: { agent: Agent; onFlash: (m: string) => void }) {
  const { rowRef, close, bind } = useSwipeRow(68)
  const unretire = useUnretireAgent()

  const onUnretire = () => {
    close()
    if (unretire.isPending) return
    unretire.mutate(agent.id, {
      onSuccess: () => onFlash(`Bringing ${agent.name} back — it will reconnect in a moment`),
      onError: (e) => onFlash(e instanceof Error ? e.message : `Could not unretire ${agent.name}`),
    })
  }

  return (
    <div className="relative overflow-hidden">
      <div className="absolute inset-y-0 right-0 flex" style={{ width: 68 }}>
        <button
          onClick={onUnretire}
          disabled={unretire.isPending}
          className="flex w-full flex-col items-center justify-center gap-0.5 bg-(--interactive) text-[11px] font-medium text-white disabled:opacity-60"
        >
          {unretire.isPending ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <ArchiveRestore className="size-4" />
          )}
          Unretire
        </button>
      </div>
      <div
        ref={rowRef}
        {...bind}
        className="relative flex touch-pan-y items-center gap-3 bg-background px-4 py-3 select-none"
      >
        <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted/50 text-muted-foreground">
          <FolderGit2 className="size-5" />
        </span>
        <span className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className="truncate text-[16px] font-medium text-foreground/75">{agent.name}</span>
          <span className="truncate text-[12.5px] text-muted-foreground/60">{agent.folder}</span>
        </span>
        <span className="shrink-0 rounded-full bg-muted/60 px-2 py-0.5 text-[10.5px] font-medium text-muted-foreground/70">
          Retired
        </span>
      </div>
    </div>
  )
}
