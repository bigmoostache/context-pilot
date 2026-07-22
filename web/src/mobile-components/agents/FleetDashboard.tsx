import { useEffect, useRef, useState } from "react"
import { animate, createSpring, stagger } from "animejs"
import {
  AlertTriangle,
  ArchiveRestore,
  Bot,
  ChevronRight,
  FolderGit2,
  FolderPlus,
  Loader2,
  Plus,
  Settings2,
} from "lucide-react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import { accentVar, fmtCost, FLEET_MAX_W } from "@/lib/support/panelMeta"
import {
  useMetrics,
  useRetiredFleet,
  useRetireAgent,
  useUnretireAgent,
  useAgentMeta,
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

type Modal = { mode: "create" } | { mode: "manage"; agent: Agent } | null

/**
 * Fleet dashboard — mobile twin of `components/agents/FleetDashboard`, reworked
 * to a full iOS-native feel to match the threads surface (T631).
 *
 * The desktop `md:grid-cols-2` card grid becomes a **single full-width column**
 * of tappable rows: the whole card is the primary action (tap = open the agent,
 * mirroring a thread row tap opening its conversation), and the secondary
 * actions (Manage / Retire) hide behind a **swipe-left** reveal — the same
 * gesture engine (`useSwipeRow`) and iOS convention the thread list uses, so the
 * card face stays clean instead of carrying an always-visible button pair.
 *
 * A large iOS title heads the page with a top-right add glyph (Contacts-style);
 * the dashed "New agent" card only appears when the fleet is empty, as
 * onboarding. anime.js springs the cards in on first load and pops the toast.
 * Selection / create / retire / unretire logic is identical to desktop — only
 * the layout axis, touch affordances, and motion fork.
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
  const [toast, setToast] = useState<string | null>(null)

  // Honour an external "create a new agent" request from the switcher — a
  // genuine reaction to a prop edge, deferred to a microtask so it lands after
  // commit (same set-state-in-effect avoidance as desktop).
  useEffect(() => {
    if (!autoCreate) return
    queueMicrotask(() => setModal({ mode: "create" }))
    onAutoCreateConsumed?.()
  }, [autoCreate, onAutoCreateConsumed])

  const flash = (m: string) => {
    setToast(m)
    window.setTimeout(() => setToast(null), 2200)
  }

  // #1 Card cascade (anime.js): stagger the fleet cards in the first time the
  // roster lands (mount / initial fetch), for the iOS list-populate feel. A ref
  // guard runs it ONCE — creating/retiring an agent later must not re-cascade
  // the whole list (that would flicker unrelated rows). Reduced-motion skips it.
  const listRef = useRef<HTMLDivElement>(null)
  const cascadedRef = useRef(false)
  useEffect(() => {
    const el = listRef.current
    if (!el || cascadedRef.current || agents.length === 0 || prefersReducedMotion()) return
    cascadedRef.current = true
    animate(el.children, {
      opacity: [0, 1],
      translateY: [8, 0],
      delay: stagger(40),
      duration: 320,
      ease: "out(2)",
    })
  }, [agents.length])

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ScrollArea className="min-h-0 flex-1 bg-background">
        <div className={cn("mx-auto flex w-full flex-col gap-5 px-4 py-6", FLEET_MAX_W)}>
          {/* iOS large-title header + Contacts-style top-right add glyph. */}
          <header className="flex items-center justify-between gap-3">
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
              onClick={() => setModal({ mode: "create" })}
              aria-label="New agent"
              className="flex size-10 shrink-0 items-center justify-center rounded-full bg-(--interactive)/12 text-(--interactive) transition-[filter] active:brightness-110"
            >
              <Plus className="size-5.5" strokeWidth={2.5} />
            </button>
          </header>

          {/* single-column tappable card stack — swipe-left reveals actions */}
          <div ref={listRef} className="flex flex-col gap-3">
            {agents.map((a) => (
              <AgentSwipeRow
                key={a.id}
                agent={a}
                onOpen={() => onOpenAgent(a.id)}
                onManage={() => setModal({ mode: "manage", agent: a })}
                onFlash={flash}
              />
            ))}
            {/* dashed onboarding card only when the fleet is empty */}
            {agents.length === 0 && <NewAgentCard onClick={() => setModal({ mode: "create" })} />}
          </div>

          <RetiredSection onFlash={flash} />
        </div>
      </ScrollArea>

      {modal && <AgentModal modal={modal} onClose={() => setModal(null)} onFlash={flash} />}

      <Toast message={toast} />
    </div>
  )
}

/** Pixel width of the revealed action strip (two 68px action buttons). */
const ACTION_W = 136

/**
 * A fleet card wrapped so a **left-swipe** reveals its trailing actions (Manage /
 * Retire) — the native iOS list gesture, reusing the shared {@link useSwipeRow}
 * engine (direct-DOM-write drag, axis lock, pointer capture, velocity flick,
 * spring snap). The card face itself is the primary tap target (open the agent);
 * the swipe strip carries the secondary actions the desktop card showed as a
 * permanent button row.
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
    <div className="relative overflow-hidden rounded-xl">
      {/* action strip — behind the card, pinned to the right edge */}
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

      {/* Card content — slides left on swipe. `touch-pan-y` keeps native vertical
          scroll while the hook owns the horizontal drag; the transform is written
          directly to this node (never via a React `style` prop) so a drag causes
          zero re-renders. Tapping while open just closes. */}
      <div ref={rowRef} {...bind} className="relative touch-pan-y select-none">
        <AgentCard agent={agent} onOpen={onOpen} />
      </div>
    </div>
  )
}

/** The tappable fleet card face — whole card opens the agent (T631). Live vitals
 *  (status dot, cost) ride the per-agent meta cache; the polled fleet row is the
 *  fallback until the first delta. A trailing chevron signals it's tappable. */
function AgentCard({ agent, onOpen }: { agent: Agent; onOpen: () => void }) {
  const { data: live } = useAgentMeta(agent.id)
  const a = live ?? agent
  const s = statusMeta[a.status]
  const accent = accentVar[a.accent]

  return (
    <button
      onClick={onOpen}
      className="card-shadow flex w-full flex-col gap-3 rounded-xl border border-border bg-card p-4 text-left transition-colors active:bg-muted/40"
    >
      <div className="flex items-center gap-3">
        {a.hasAvatar ? (
          <img
            src={avatarUrl(agent.id)}
            alt={agent.name}
            className="size-10 shrink-0 rounded-lg object-cover"
          />
        ) : (
          <span
            className="flex size-10 shrink-0 items-center justify-center rounded-lg"
            style={{ background: `color-mix(in oklab, ${accent} 16%, transparent)`, color: accent }}
          >
            <FolderGit2 className="size-5" />
          </span>
        )}
        <div className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className="truncate text-[15px] font-semibold text-foreground/90">
            {agent.name}
          </span>
        </div>
        <span
          className="inline-flex shrink-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[10.5px] font-medium"
          style={{ background: `color-mix(in oklab, ${s.color} 14%, transparent)`, color: s.color }}
        >
          <span
            className={cn("size-1.5 rounded-full", a.status === "working" && "animate-pulse")}
            style={{ background: s.color }}
          />
          {s.label}
        </span>
        <ChevronRight className="size-4 shrink-0 text-muted-foreground/30" />
      </div>

      {/* §19 health — a degraded stream / lagging projection surfaces here so
          it is VISIBLE, never a silent backend latch (T121). */}
      <HealthBadge agentId={agent.id} />

      <p className="line-clamp-2 min-h-[2.4em] text-[12.5px] leading-snug text-foreground/70">
        {agent.task}
      </p>

      <div className="flex items-center gap-4 text-[11.5px] text-muted-foreground">
        <span className="inline-flex items-center gap-1">
          <Bot className="size-3.5" />
          {agent.model}
        </span>
        <span className="ml-auto font-semibold text-foreground/80 tabular-nums">
          {fmtCost(a.costUsd)}
        </span>
      </div>
    </button>
  )
}

/** Rev-lag threshold above which the projection is flagged as falling behind. */
const REV_LAG_WARN = 50

/** The first non-nominal health condition for an agent card, or null when all
 *  nominal. Flat if-chain: degraded stream first, then lagging projection. */
function healthCondition(
  data: NonNullable<ReturnType<typeof useMetrics>["data"]>,
): { label: string; tone: string; title: string } | null {
  const { stream, rev } = data
  if (stream.degraded) {
    return {
      label: "Stream degraded",
      tone: "var(--warn)",
      title: `Live token stream dropped ${stream.droppedFrames} frame(s) — a slow consumer is being shed (the durable record is unaffected).`,
    }
  }
  if ((rev.lag ?? 0) > REV_LAG_WARN) {
    return {
      label: "Projection lagging",
      tone: "var(--warn)",
      title: `Backend view is ${rev.lag} revs behind the oplog head (view ${rev.view} / head ${rev.oplogHead ?? "?"}). The projection is falling behind the durable log.`,
    }
  }
  return null
}

/** §19 health badge — surfaces the first non-nominal condition as a coloured
 *  pill so a degraded stream / lagging projection is visible at a glance. */
function HealthBadge({ agentId }: { agentId: string }) {
  const { data } = useMetrics(agentId)
  if (!data) return null

  const condition = healthCondition(data)
  if (!condition) return null

  return (
    <span
      role="status"
      title={condition.title}
      className="inline-flex w-fit items-center gap-1.5 rounded-md px-2 py-0.5 text-[10.5px] font-medium"
      style={{
        background: `color-mix(in oklab, ${condition.tone} 14%, transparent)`,
        color: condition.tone,
      }}
    >
      <AlertTriangle className="size-3" />
      {condition.label}
    </span>
  )
}

/** Retired (archived) agents section — single-column on mobile, shown only when
 *  at least one agent is retired (T271). Retired agents have no live process, so
 *  the card is not swipe/tap-to-open; a single Unretire button restores it. */
function RetiredSection({ onFlash }: { onFlash: (m: string) => void }) {
  const { data: retired } = useRetiredFleet()
  if (!retired || retired.length === 0) return null

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <h2 className="text-[12.5px] font-semibold tracking-[0.06em] text-muted-foreground/80 uppercase">
          Retired
        </h2>
        <span className="rounded-full bg-muted/60 px-1.5 py-0.5 text-[10.5px] font-medium text-muted-foreground/70">
          {retired.length}
        </span>
      </div>
      <div className="flex flex-col gap-3">
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
    <div className="flex flex-col gap-3 rounded-xl border border-dashed border-border bg-card/50 p-4">
      <div className="flex items-center gap-3">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted/50 text-muted-foreground">
          <FolderGit2 className="size-5" />
        </span>
        <div className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className="truncate text-[15px] font-semibold text-foreground/75">
            {agent.name}
          </span>
          <span className="truncate text-[11px] text-muted-foreground/60">{agent.folder}</span>
        </div>
        <span className="inline-flex shrink-0 items-center rounded-full bg-muted/60 px-2 py-0.5 text-[10.5px] font-medium text-muted-foreground/70">
          Retired
        </span>
      </div>

      <button
        onClick={onUnretire}
        disabled={unretire.isPending}
        className="mt-0.5 flex items-center justify-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-[13.5px] font-medium text-foreground/70 transition-colors active:border-(--interactive)/50 active:text-(--interactive) disabled:cursor-not-allowed disabled:opacity-50"
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

function NewAgentCard({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="flex min-h-[128px] flex-col items-center justify-center gap-2.5 rounded-xl border border-dashed border-border bg-transparent p-4 text-muted-foreground transition-colors active:border-(--interactive)/60 active:text-(--interactive)"
    >
      <span className="flex size-11 items-center justify-center rounded-xl bg-muted/50">
        <FolderPlus className="size-5" />
      </span>
      <span className="text-[13.5px] font-medium">New agent</span>
      <span className="max-w-[240px] text-center text-[11.5px] text-muted-foreground/60">
        Initialize an agent in a folder — its realm for the whole session.
      </span>
    </button>
  )
}

/**
 * The transient action toast — springs in from below (anime.js) for a livelier
 * confirmation than a flat appear. Renders nothing when there's no message; the
 * spring fires on each mount (the element is conditionally rendered, so a new
 * message remounts it). Reduced-motion shows it at rest.
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
