import { useEffect, useRef, useState } from "react"
import { animate, createSpring, stagger } from "animejs"
import {
  AlertTriangle,
  ArchiveRestore,
  ChevronRight,
  FolderGit2,
  Loader2,
  Plus,
  Search,
  Settings2,
  X,
} from "lucide-react"
import { ScrollArea } from "@/mobile-components/ui/scroll-area"
import { FrostedBottomBar } from "@/mobile-components/shell/FrostedBottomBar"
import { useElementHeight } from "@/lib/live/useElementHeight"
import { accentVar, fmtCost, FLEET_MAX_W } from "@/lib/support/panelMeta"
import {
  useMetrics,
  useRetireAgent,
  useAgentMeta,
  useCreateAgent,
} from "@/lib/live"
import { avatarUrl } from "@/lib/api"
import type { Agent, AgentStatus } from "@/lib/types"
import { cn, prefersReducedMotion } from "@/lib/utils"
import { useSwipeRow } from "@/lib/live/useSwipeRow"
import { RetiredSection } from "./FleetRetired"

const statusMeta: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working", color: "var(--interactive)" },
  "needs-you": { label: "Needs input", color: "var(--signal)" },
  idle: { label: "Standby", color: "var(--muted-foreground)" },
  disconnected: { label: "Disconnected", color: "var(--danger)" },
  waiting: { label: "Restarting", color: "var(--interactive)" },
}

/**
 * Fleet dashboard — mobile twin of `components/agents/FleetDashboard`, rebuilt
 * as an **iOS list of items** (thread-list pattern) rather than a card grid
 * (T632): a flat borderless list where each agent is a full-width tappable row
 * (leading avatar/icon with a status presence-dot, name, `model · task`
 * subtitle, trailing cost + chevron). Tap opens the agent; swipe-left reveals
 * Manage / Retire (shared `useSwipeRow`).
 *
 * Creation mirrors the mobile ThreadList (T633): the **bottom bar field doubles
 * as search and the create-name** — typing filters the roster live AND is the
 * draft name for a new agent; a round `+` button appears only once the field is
 * non-empty, and tapping it (or pressing Return) spawns an agent with that name
 * (folder derived server-side, model defaulted — change it later via Manage)
 * then clears the field. This replaced the earlier full-screen `AgentModal`
 * create flow and the intermediate `NewAgentSheet` bottom sheet.
 */
export function FleetDashboard({
  agents,
  onOpenAgent,
  onManageAgent,
  autoCreate,
  onAutoCreateConsumed,
}: {
  agents: Agent[]
  onOpenAgent: (id: string) => void
  onManageAgent: (id: string) => void
  autoCreate?: boolean | undefined
  onAutoCreateConsumed?: (() => void) | undefined
}) {
  const [query, setQuery] = useState("")
  const [toast, setToast] = useState<string | null>(null)
  const createAgent = useCreateAgent()
  const inputRef = useRef<HTMLInputElement>(null)
  // Floating glass bottom bar (T637): reserve a 1.5× spacer sized from its
  // measured height so the last row scrolls clear of the frosted bar.
  const barRef = useRef<HTMLDivElement>(null)
  const barH = useElementHeight(barRef)

  const flash = (m: string) => {
    setToast(m)
    window.setTimeout(() => setToast(null), 2200)
  }

  /** Spawn an agent named after the current field value, then clear it. Folder
   *  is derived server-side from the name; the model defaults (set later via
   *  Manage). No-op on an empty field or while a create is already in flight. */
  const create = () => {
    const name = query.trim()
    if (name === "" || createAgent.isPending) return
    createAgent.mutate(
      { name },
      {
        onSuccess: (receipt) => {
          flash(`Spawning “${name}” in ${receipt.folder}`)
          setQuery("")
        },
        onError: (err) =>
          flash(err instanceof Error ? err.message : "Could not create the agent"),
      },
    )
  }

  // Honour an external "create a new agent" request from the switcher — a
  // genuine reaction to a prop edge, deferred to a microtask so it lands after
  // commit (set-state-in-effect avoidance). There's no create sheet anymore, so
  // this focuses the dual-use bottom field (the create surface) instead.
  useEffect(() => {
    if (!autoCreate) return
    queueMicrotask(() => inputRef.current?.focus())
    onAutoCreateConsumed?.()
  }, [autoCreate, onAutoCreateConsumed])

  // Live filter: the field doubles as search, so the roster narrows as the user
  // types (matching name substring, case-insensitive). Whatever is typed is ALSO
  // the draft name for the create button below — same dual-use as ThreadList.
  const q = query.trim().toLowerCase()
  const filtered = q === "" ? agents : agents.filter((a) => a.name.toLowerCase().includes(q))

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
          </header>

          {agents.length === 0 ? (
            <p className="px-4 py-16 text-center text-[14px] text-muted-foreground/55">
              No agents yet — type a name below to create one.
            </p>
          ) : filtered.length === 0 ? (
            <p className="px-4 py-16 text-center text-[14px] text-muted-foreground/55">
              No agents match “{query.trim()}”.
            </p>
          ) : (
            <ul ref={listRef} className="flex flex-col">
              {filtered.map((a) => (
                <li key={a.id}>
                  <AgentSwipeRow
                    agent={a}
                    onOpen={() => onOpenAgent(a.id)}
                    onManage={() => onManageAgent(a.id)}
                    onFlash={flash}
                  />
                </li>
              ))}
            </ul>
          )}

          <RetiredSection onFlash={flash} />

          {/* Bottom spacer = 1.5× the floating bar height, so the last row can
              always scroll clear of the frosted glass bar (T637). */}
          <div aria-hidden style={{ height: barH * 1.5 }} />
        </div>
      </ScrollArea>

      {/* Bottom action bar — the search field DOUBLES as the create input (T633,
          mirroring ThreadList): typing filters the roster live AND is the draft
          name for a new agent. A create button appears only once the field is
          non-empty; tapping it — or pressing Return — spawns an agent with that
          name and clears the field. */}
      <FrostedBottomBar
        ref={barRef}
        className="flex items-center gap-2 px-3 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]"
      >
        <div className="flex flex-1 items-center gap-2 rounded-xl bg-muted/60 px-3 py-2 text-[16px]">
          <Search className="size-4 shrink-0 text-muted-foreground/60" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              // Return spawns an agent named after the current value — the mobile
              // keyboard's "go" affordance for the dual-use field. No-op empty.
              if (e.key !== "Enter" || query.trim() === "") return
              e.preventDefault()
              create()
            }}
            placeholder="Search or create an agent"
            className="min-w-0 flex-1 bg-transparent text-foreground/90 outline-none placeholder:text-muted-foreground/55"
          />
          {query && (
            <button
              onClick={() => setQuery("")}
              className="shrink-0 text-muted-foreground/55 active:text-foreground"
              title="Clear"
            >
              <X className="size-4" />
            </button>
          )}
        </div>
        {/* Create — appears only when the field is non-empty: the field's value
            is the new agent's name. */}
        {query.trim() !== "" && (
          <button
            onClick={create}
            disabled={createAgent.isPending}
            aria-label="Create agent"
            className="flex size-11 shrink-0 items-center justify-center rounded-full bg-(--interactive) text-(--primary-foreground) transition-[filter] active:brightness-110 disabled:opacity-60"
          >
            {createAgent.isPending ? (
              <Loader2 className="size-5 animate-spin" />
            ) : (
              <Plus className="size-5" />
            )}
          </button>
        )}
      </FrostedBottomBar>

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
