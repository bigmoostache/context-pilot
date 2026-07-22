import { useEffect, useRef, useState } from "react"
import { animate, createSpring, stagger } from "animejs"
import {
  AlertTriangle,
  ArchiveRestore,
  ChevronRight,
  FolderGit2,
  Loader2,
  Plus,
  Settings2,
} from "lucide-react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import { Dialog, DialogContent, DialogTitle } from "@/mobile-components/ui/dialog"
import { accentVar, fmtCost, FLEET_MAX_W } from "@/lib/support/panelMeta"
import {
  useMetrics,
  useRetiredFleet,
  useRetireAgent,
  useUnretireAgent,
  useAgentMeta,
  useCreateAgent,
} from "@/lib/live"
import { avatarUrl } from "@/lib/api"
import type { Agent, AgentStatus } from "@/lib/types"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { useSwipeRow } from "@/lib/live/useSwipeRow"
import { AgentModal } from "./AgentModal"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs input", color: "var(--signal)" },
  idle: { label: "Standby", color: "var(--muted-foreground)" },
  disconnected: { label: "Disconnected", color: "var(--danger)" },
  waiting: { label: "Restarting", color: "var(--interactive)" },
}

type Modal = { mode: "manage"; agent: Agent } | null

/**
 * Fleet dashboard — mobile twin of `components/agents/FleetDashboard`, rebuilt
 * as an **iOS list of items** (thread-list pattern) rather than a card grid
 * (T632): a flat borderless list where each agent is a full-width tappable row
 * (leading avatar/icon with a status presence-dot, name, `model · task`
 * subtitle, trailing cost + chevron). Tap opens the agent; swipe-left reveals
 * Manage / Retire (shared `useSwipeRow`). The `+` opens a light {@link
 * NewAgentSheet} bottom sheet (single name field, model defaults, set later via
 * Manage) instead of the heavy full-screen `AgentModal`. anime.js cascades the
 * rows in on first load and pops the toast.
 */
export function FleetDashboard({
  agents,
  onOpenAgent,
  autoCreate,
  onAutoCreateConsumed,
}: {
  agents: Agent[]
  onOpenAgent: (id: string) => void
  autoCreate?: boolean | undefined
  onAutoCreateConsumed?: (() => void) | undefined
}) {
  const [modal, setModal] = useState<Modal>(null)
  const [creating, setCreating] = useState(false)
  const [toast, setToast] = useState<string | null>(null)

  // Honour an external "create a new agent" request from the switcher — a
  // genuine reaction to a prop edge, deferred to a microtask so it lands after
  // commit (same set-state-in-effect avoidance as desktop).
  useEffect(() => {
    if (!autoCreate) return
    queueMicrotask(() => setCreating(true))
    onAutoCreateConsumed?.()
  }, [autoCreate, onAutoCreateConsumed])

  const flash = (m: string) => {
    setToast(m)
    window.setTimeout(() => setToast(null), 2200)
  }

  // #1 List cascade (anime.js): stagger the rows in the first time the roster
  // lands. A ref guard runs it ONCE — creating/retiring later must not re-cascade
  // the whole list (that would flicker unrelated rows). Reduced-motion skips it.
  const listRef = useRef<HTMLUListElement>(null)
  const cascadedRef = useRef(false)
  useEffect(() => {
    const el = listRef.current
    if (!el || cascadedRef.current || agents.length === 0 || prefersReducedMotion()) return
    cascadedRef.current = true
    animate(el.children, {
      opacity: [0, 1],
      translateY: [8, 0],
      delay: stagger(35),
      duration: 300,
      ease: "out(2)",
    })
  }, [agents.length])

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ScrollArea className="min-h-0 flex-1 bg-background">
        <div className={cn("mx-auto flex w-full flex-col", FLEET_MAX_W)}>
          <header className="flex items-center justify-between gap-3 px-4 pt-6 pb-3">
            <div className="flex flex-col gap-0.5">
              <h1 className="text-[28px] leading-none font-bold tracking-tight text-foreground">
                Agents
              </h1>
              {agents.length > 0 && (
                <span className="text-[12.5px] text-muted-foreground/70">
                  {agents.length} {agents.length === 1 ? "agent" : "agents"}
                </span>
              )}
            </div>
            <button
              onClick={() => setCreating(true)}
              aria-label="New agent"
              className="flex size-10 shrink-0 items-center justify-center rounded-full bg-(--interactive)/12 text-(--interactive) transition-[filter] active:brightness-110"
            >
              <Plus className="size-5.5" strokeWidth={2.5} />
            </button>
          </header>

          {agents.length === 0 ? (
            <p className="px-4 py-16 text-center text-[14px] text-muted-foreground/55">
              No agents yet — tap + to create one.
            </p>
          ) : (
            <ul ref={listRef} className="flex flex-col">
              {agents.map((a) => (
                <li key={a.id}>
                  <AgentSwipeRow
                    agent={a}
                    onOpen={() => onOpenAgent(a.id)}
                    onManage={() => setModal({ mode: "manage", agent: a })}
                    onFlash={flash}
                  />
                </li>
              ))}
            </ul>
          )}

          <RetiredSection onFlash={flash} />
        </div>
      </ScrollArea>

      {modal && <AgentModal modal={modal} onClose={() => setModal(null)} onFlash={flash} />}

      <NewAgentSheet open={creating} onClose={() => setCreating(false)} onFlash={flash} />

      <Toast message={toast} />
    </div>
  )
}

/** Pixel width of the revealed action strip (two 68px action buttons). */
const ACTION_W = 136

/**
 * An agent row wrapped so a **left-swipe** reveals its Manage / Retire actions —
 * the native iOS list gesture via the shared {@link useSwipeRow} engine. The row
 * face itself is the primary tap target (open the agent).
 */
function AgentSwipeRow({
  agent,
  onOpen,
  onManage,
  onFlash,
}: {
  agent: Agent
  onOpen: () => void
  onManage: () => void
  onFlash: (m: string) => void
}) {
  const { rowRef, close, bind } = useSwipeRow(ACTION_W)
  const retire = useRetireAgent()

  const onRetire = () => {
    close()
    if (retire.isPending) return
    retire.mutate(agent.id, {
      onSuccess: () => onFlash(`Retiring ${agent.name} — its folder is kept`),
      onError: (e) => onFlash(e instanceof Error ? e.message : `Could not retire ${agent.name}`),
    })
  }

  return (
    <div className="relative overflow-hidden">
      {/* action strip — behind the row, pinned to the right edge */}
      <div className="absolute inset-y-0 right-0 flex" style={{ width: ACTION_W }}>
        <button
          onClick={() => {
            onManage()
            close()
          }}
          className="flex w-1/2 flex-col items-center justify-center gap-0.5 bg-(--interactive) text-[11px] font-medium text-white"
        >
          <Settings2 className="size-4" />
          Manage
        </button>
        <button
          onClick={onRetire}
          className="flex w-1/2 flex-col items-center justify-center gap-0.5 bg-(--warn) text-[11px] font-medium text-white"
        >
          <ArchiveRestore className="size-4" />
          Retire
        </button>
      </div>

      {/* Row content — slides left on swipe. `touch-pan-y` keeps native vertical
          scroll while the hook owns the horizontal drag; the transform is written
          directly to this node (never via a React `style` prop) so a drag causes
          zero re-renders. Tapping while open just closes. */}
      <div ref={rowRef} {...bind} className="relative touch-pan-y bg-background select-none">
        <AgentRow agent={agent} onOpen={onOpen} />
      </div>
    </div>
  )
}

/**
 * A single fleet row (thread-list item styling): leading avatar/icon with a
 * status **presence dot**, name (bold when needs-you), `model · task` subtitle,
 * trailing cost + chevron. Live vitals ride the per-agent meta cache; the polled
 * fleet row is the fallback until the first delta. Whole row taps open (T632).
 */
function AgentRow({ agent, onOpen }: { agent: Agent; onOpen: () => void }) {
  const { data: live } = useAgentMeta(agent.id)
  const a = live ?? agent
  const s = statusMeta[a.status]
  const accent = accentVar[a.accent]
  const attention = a.status === "needs-you"
  const subtitle = [agent.model, agent.task].filter(Boolean).join(" · ")

  return (
    <button
      onClick={onOpen}
      className="flex w-full items-center gap-3 px-4 py-3 text-left active:bg-muted/40"
    >
      <span className="relative shrink-0">
        {a.hasAvatar ? (
          <img
            src={avatarUrl(agent.id)}
            alt={agent.name}
            className="size-10 rounded-lg object-cover"
          />
        ) : (
          <span
            className="flex size-10 items-center justify-center rounded-lg"
            style={{ background: `color-mix(in oklab, ${accent} 16%, transparent)`, color: accent }}
          >
            <FolderGit2 className="size-5" />
          </span>
        )}
        <span
          title={s.label}
          className={cn(
            "absolute -right-0.5 -bottom-0.5 size-3 rounded-full ring-2 ring-background",
            a.status === "working" && "animate-pulse",
          )}
          style={{ background: s.color }}
        />
      </span>

      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-baseline gap-2">
          <span
            className={cn(
              "truncate text-[16px] text-foreground",
              attention ? "font-semibold" : "font-medium",
            )}
          >
            {agent.name}
          </span>
          <HealthDot agentId={agent.id} />
          <span className="ml-auto shrink-0 text-[12.5px] font-semibold text-foreground/75 tabular-nums">
            {fmtCost(a.costUsd)}
          </span>
          <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/30" />
        </span>
        <span className="truncate text-[13px] leading-snug text-muted-foreground/70">
          {subtitle || s.label}
        </span>
      </span>
    </button>
  )
}

/** Rev-lag threshold above which the projection is flagged as falling behind. */
const REV_LAG_WARN = 50

/** The first non-nominal health condition for an agent, or null when all
 *  nominal (degraded stream first, then lagging projection). */
function healthCondition(data: NonNullable<ReturnType<typeof useMetrics>["data"]>): string | null {
  const { stream, rev } = data
  if (stream.degraded)
    return `Live token stream dropped ${stream.droppedFrames} frame(s) — a slow consumer is being shed (the durable record is unaffected).`
  if ((rev.lag ?? 0) > REV_LAG_WARN)
    return `Backend view is ${rev.lag} revs behind the oplog head (view ${rev.view} / head ${rev.oplogHead ?? "?"}). The projection is falling behind the durable log.`
  return null
}

/**
 * §19 health indicator — a small amber warning glyph beside the name (the item
 * row has no room for the desktop pill). Surfaces only on a non-nominal
 * condition, so a silent backend latch stays visible (T121); the full
 * explanation rides the `title` tooltip.
 */
function HealthDot({ agentId }: { agentId: string }) {
  const { data } = useMetrics(agentId)
  if (!data || !healthCondition(data)) return null
  return <AlertTriangle className="size-3.5 shrink-0 text-(--warn)" aria-label="Health warning" />
}

/** Retired (archived) agents — same item styling, muted, shown only when at
 *  least one is retired (T271). A retired agent has no live process to open, so
 *  the row isn't tap-to-open; a swipe-left reveals its single Unretire action. */
function RetiredSection({ onFlash }: { onFlash: (m: string) => void }) {
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

/**
 * Light bottom-sheet agent creation (T632) — replaces the heavy full-screen
 * `AgentModal` create flow, mirroring the New-Thread sheet the user preferred:
 * grabber, `Cancel · New Agent · Create` nav bar, one autofocused 16px name
 * field (16px defeats iOS focus-zoom). The model defaults server-side (change it
 * later via Manage), so creation is one field, one tap. Submitting spawns the
 * agent (folder derived from the name) and flashes the receipt.
 */
function NewAgentSheet({
  open,
  onClose,
  onFlash,
}: {
  open: boolean
  onClose: () => void
  onFlash: (m: string) => void
}) {
  const [name, setName] = useState("")
  const createAgent = useCreateAgent()
  const canCreate = name.trim().length > 0 && !createAgent.isPending

  const close = () => {
    setName("")
    onClose()
  }

  const submit = (e: React.SyntheticEvent) => {
    e.preventDefault()
    if (!canCreate) return
    createAgent.mutate(
      { name: name.trim() },
      {
        onSuccess: (receipt) => {
          onFlash(`Spawning “${name.trim()}” in ${receipt.folder}`)
          close()
        },
        onError: (err) =>
          onFlash(err instanceof Error ? err.message : "Could not create the agent"),
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="px-0 pt-2 pb-[max(1.25rem,env(safe-area-inset-bottom))]">
        <div className="mx-auto mb-1 h-1 w-9 rounded-full bg-muted-foreground/25" />

        {/* iOS nav-bar header: Cancel · title · Create */}
        <div className="grid grid-cols-[1fr_auto_1fr] items-center border-b border-border/70 px-4 py-2">
          <button
            type="button"
            onClick={close}
            className="justify-self-start text-[16px] text-(--interactive) active:opacity-60"
          >
            Cancel
          </button>
          <DialogTitle className="justify-self-center text-[16px]">New Agent</DialogTitle>
          <button
            type="submit"
            form="new-agent-form"
            disabled={!canCreate}
            className="justify-self-end text-[16px] font-semibold text-(--interactive) active:opacity-60 disabled:text-muted-foreground/40"
          >
            Create
          </button>
        </div>

        <form id="new-agent-form" onSubmit={submit} className="px-4 pt-4">
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Agent name"
            className="w-full rounded-xl bg-muted/60 px-4 py-3 text-[16px] text-foreground/90 outline-none placeholder:text-muted-foreground/50"
          />
          <p className="mt-2 px-1 text-[12.5px] text-muted-foreground/70">
            Spawns an agent in a folder named after it. Pick its model later via Manage.
          </p>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/**
 * The transient action toast — springs in from below (anime.js). Renders
 * nothing when there's no message; the element is conditionally rendered, so a
 * new message remounts it and the spring re-fires. Reduced-motion shows at rest.
 */
function Toast({ message }: { message: string | null }) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = ref.current
    if (!el || prefersReducedMotion()) return
    animate(el, {
      translateY: [16, 0],
      opacity: [0, 1],
      ease: createSpring({ stiffness: 420, damping: 30 }),
    })
  }, [])
  if (message === null) return null
  return (
    <div
      ref={ref}
      className="pop-shadow absolute bottom-6 left-1/2 z-30 -translate-x-1/2 rounded-lg border border-border bg-card px-4 py-2 text-[12px] text-foreground/90"
    >
      {message}
    </div>
  )
}
