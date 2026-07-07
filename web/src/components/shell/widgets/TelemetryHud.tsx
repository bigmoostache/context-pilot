import { useState } from "react"
import { Activity, X, Trash2, ChevronDown, ChevronUp, Copy, Check } from "lucide-react"
import { useDevMode } from "@/lib/providers/devMode"
import {
  useTelemetry,
  resetTelemetry,
  type TelemetrySnapshot,
  type LoafEvent,
  type CommitEvent,
  type StallEvent,
  type TaskEvent,
  type BlockEvent,
  type TaskAgg,
} from "@/lib/support/telemetry"
import { cn, clipboard } from "@/lib/utils"

/**
 * Dev-mode performance HUD — a live, corner-docked readout of where the user's
 * wall-time is going, gated behind the same Developer-mode flag as the Cockpit.
 * It surfaces the three telemetry signals at a glance so a lag/freeze can be
 * diagnosed without opening browser DevTools:
 *   • Core Web Vitals (INP headline — the responsiveness metric — plus LCP/CLS),
 *   • the worst Long Animation Frames with their culprit script, and
 *   • the worst React commits by subtree.
 * It reads the coalesced telemetry snapshot (throttled in the store) so the HUD
 * itself never becomes a source of the render churn it measures.
 */
export function TelemetryHud() {
  const { devMode } = useDevMode()
  const [open, setOpen] = useState(true)
  const [collapsed, setCollapsed] = useState(false)
  const [copied, setCopied] = useState(false)
  const snap = useTelemetry()

  if (!devMode || !open) return null

  const inp = snap.vitals["INP"]
  const lcp = snap.vitals["LCP"]
  const cls = snap.vitals["CLS"]

  // Copy the full snapshot as markdown so it can be pasted straight into a
  // thread for diagnosis. The report carries MORE than the HUD renders (every
  // worst-list entry, the counts/totals, and the userAgent — so the browser is
  // known without asking). A transient check confirms the copy.
  const doCopy = () => {
    const cb = clipboard()
    if (!cb) return
    void cb.writeText(snapshotToMarkdown(snap))
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1500)
  }

  return (
    <div className="fixed bottom-4 right-4 z-[60] flex max-h-[calc(100vh-2rem)] w-[320px] flex-col rounded-xl border border-border bg-popover/95 text-[12px] text-foreground pop-shadow backdrop-blur-md">
      <header className="flex shrink-0 items-center gap-2 border-b border-border/70 px-3 py-2">
        <Activity className="size-3.5 text-[var(--signal)]" />
        <span className="font-semibold tracking-tight">Performance</span>
        <span className="ml-auto flex items-center gap-1">
          <IconBtn title={copied ? "Copied!" : "Copy as markdown"} onClick={doCopy}>
            {copied ? (
              <Check className="size-3.5 text-[var(--ok)]" />
            ) : (
              <Copy className="size-3.5" />
            )}
          </IconBtn>
          <IconBtn title="Clear" onClick={() => resetTelemetry()}>
            <Trash2 className="size-3.5" />
          </IconBtn>
          <IconBtn
            title={collapsed ? "Expand" : "Collapse"}
            onClick={() => setCollapsed((v) => !v)}
          >
            {collapsed ? <ChevronUp className="size-3.5" /> : <ChevronDown className="size-3.5" />}
          </IconBtn>
          <IconBtn title="Close" onClick={() => setOpen(false)}>
            <X className="size-3.5" />
          </IconBtn>
        </span>
      </header>

      {!collapsed && (
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-3 py-2.5">
          {/* Real main-thread blocks — the AUTHORITATIVE freeze signal. A Web
              Worker heartbeat runs on its own (never-throttled) thread; a gap
              here means the main thread was GENUINELY blocked, so this can't be
              faked by Firefox's focus-throttling of rAF the way a bare "stall"
              can. If a block matches a stall → real freeze; if a stall shows
              but NO block → that stall was a throttling artifact. */}
          <Group
            title="Real blocks (worker)"
            meta={`${snap.blockCount} · worst ${snap.worstBlocks[0]?.blocked ?? 0}ms`}
          >
            {snap.worstBlocks.length === 0 ? (
              <Empty>No real main-thread blocks — reproduce the freeze to capture it.</Empty>
            ) : (
              snap.worstBlocks.slice(0, 5).map((b, i) => <BlockRow key={i} block={b} />)
            )}
          </Group>

          <Divider />

          {/* Task TOTALS — the burst-catcher. A storm of individually-cheap ops
              (e.g. hundreds of sub-10ms SSE applies) never trips a single named
              entry, but sums to a freeze here: a label with a huge count/total
              names a death-by-a-thousand-cuts block. */}
          <Group title="Task totals (Σ)" meta={`${snap.taskAgg.length} labels`}>
            {snap.taskAgg.length === 0 ? (
              <Empty>No tasks yet — reproduce the freeze to attribute it.</Empty>
            ) : (
              snap.taskAgg.slice(0, 6).map((a, i) => <AggRow key={i} agg={a} />)
            )}
          </Group>

          <Divider />

          {/* Main-thread stalls — THE headline freeze signal. A multi-second
              hang shows here as one big gap even when INP/LoAF/Long-Tasks stay
              silent (those are Chromium-only / per-frame; this rAF watchdog is
              universal and catches storms of cheap tasks too). */}
          <Group
            title="Main-thread stalls"
            meta={`${snap.stallCount} · worst ${snap.worstStalls[0]?.gap ?? 0}ms`}
          >
            {snap.worstStalls.length === 0 ? (
              <Empty>No stalls yet — reproduce the freeze to capture it.</Empty>
            ) : (
              snap.worstStalls.slice(0, 5).map((s, i) => <StallRow key={i} stall={s} />)
            )}
          </Group>

          <Divider />

          {/* Named tasks — the ATTRIBUTION for a stall. A stall says the main
              thread blocked for N ms; a matching task entry (sse:apply,
              threads:merge, sse:parse) says WHICH instrumented path burned it —
              the only way to name the culprit on Firefox (no LoAF). */}
          <Group
            title="Named tasks"
            meta={`${snap.taskCount} · worst ${snap.worstTasks[0]?.duration ?? 0}ms`}
          >
            {snap.worstTasks.length === 0 ? (
              <Empty>No named blocks yet — reproduce the freeze to attribute it.</Empty>
            ) : (
              snap.worstTasks.slice(0, 5).map((t, i) => <TaskRow key={i} task={t} />)
            )}
          </Group>

          <Divider />

          {/* Web Vitals row */}
          <section className="flex items-center gap-2">
            <Vital label="INP" value={inp?.value} rating={inp?.rating} unit="ms" />
            <Vital label="LCP" value={lcp?.value} rating={lcp?.rating} unit="ms" />
            <Vital label="CLS" value={cls?.value} rating={cls?.rating} />
          </section>
          {inp?.detail && (
            <p className="-mt-1 truncate text-[10.5px] text-muted-foreground" title={inp.detail}>
              worst interaction: {inp.detail}
            </p>
          )}

          <Divider />

          {/* Long Animation Frames */}
          <Group
            title="Long frames"
            meta={`${snap.worstFrames.length} · ${Math.round(snap.totalBlockingMs)}ms blocking`}
          >
            {snap.worstFrames.length === 0 ? (
              <Empty>No long frames yet — interact to sample.</Empty>
            ) : (
              snap.worstFrames.slice(0, 5).map((f, i) => <FrameRow key={i} frame={f} />)
            )}
          </Group>

          <Divider />

          {/* React commits — a HIGH count of small commits is the SSE
              render-storm signature (many cheap re-renders, not one slow one). */}
          <Group
            title="Slow React commits"
            meta={`${snap.commitCount} commits · ${Math.round(snap.commitTotalMs)}ms · ${snap.longTaskCount} long tasks`}
          >
            {snap.worstCommits.length === 0 ? (
              <Empty>No slow commits yet.</Empty>
            ) : (
              snap.worstCommits.slice(0, 5).map((c, i) => <CommitRow key={i} commit={c} />)
            )}
          </Group>
        </div>
      )}
    </div>
  )
}

/**
 * Serialize the whole telemetry snapshot to a markdown report for pasting into
 * a thread. Deliberately richer than the HUD: every worst-list entry (not just
 * the top 5), the counts/totals, and `navigator.userAgent` (so the reader knows
 * the browser — decisive, since several signals are Chromium-only). Built as a
 * single array literal joined at the end (no repeated `.push`, which unicorn's
 * no-array-push-push would flag).
 */
function snapshotToMarkdown(snap: TelemetrySnapshot): string {
  const vitals = Object.values(snap.vitals)
  const vitalsBlock =
    vitals.length === 0
      ? "_none captured_"
      : [
          "| metric | value | rating | detail |",
          "|---|---|---|---|",
          ...vitals.map((v) => `| ${v.name} | ${v.value} | ${v.rating} | ${v.detail ?? ""} |`),
        ].join("\n")

  const stallsBlock =
    snap.worstStalls.length === 0
      ? "_none captured_"
      : snap.worstStalls
          .map((s) => `- ${s.gap}ms — blocked at ${new Date(s.ts).toLocaleTimeString()}`)
          .join("\n")

  const blocksBlock =
    snap.worstBlocks.length === 0
      ? "_none captured_"
      : snap.worstBlocks
          .map((b) => `- ${b.blocked}ms — blocked at ${new Date(b.ts).toLocaleTimeString()}`)
          .join("\n")

  const aggBlock =
    snap.taskAgg.length === 0
      ? "_none captured_"
      : [
          "| label | Σ ms | count | max ms |",
          "|---|---|---|---|",
          ...snap.taskAgg.map(
            (a) => `| ${a.label} | ${Math.round(a.total)} | ${a.count} | ${Math.round(a.max)} |`,
          ),
        ].join("\n")

  const tasksBlock =
    snap.worstTasks.length === 0
      ? "_none captured_"
      : snap.worstTasks
          .map((t) => `- ${t.duration}ms — ${t.label} (at ${new Date(t.ts).toLocaleTimeString()})`)
          .join("\n")

  const framesBlock =
    snap.worstFrames.length === 0
      ? "_none captured_"
      : snap.worstFrames
          .map(
            (f) =>
              `- ${f.duration}ms (blocking ${f.blockingDuration}ms) — ${f.script ?? "(no script)"}`,
          )
          .join("\n")

  const commitsBlock =
    snap.worstCommits.length === 0
      ? "_none captured_"
      : snap.worstCommits.map((c) => `- ${c.actualDuration}ms — ${c.id} · ${c.phase}`).join("\n")

  return [
    "## Performance telemetry",
    `- captured: ${new Date().toISOString()}`,
    `- userAgent: ${navigator.userAgent}`,
    "",
    `### Real main-thread blocks (worker heartbeat) — ${snap.blockCount} total, worst ${snap.worstBlocks[0]?.blocked ?? 0}ms`,
    "_(throttle-immune: a block here is a GENUINE freeze, not an rAF artifact)_",
    blocksBlock,
    "",
    `### Task totals (Σ per label — burst-catcher) — ${snap.taskAgg.length} labels`,
    aggBlock,
    "",
    `### Main-thread stalls (rAF) — ${snap.stallCount} total, worst ${snap.worstStalls[0]?.gap ?? 0}ms`,
    stallsBlock,
    "",
    `### Named tasks — ${snap.taskCount} total, worst ${snap.worstTasks[0]?.duration ?? 0}ms`,
    tasksBlock,
    "",
    "### Web Vitals",
    vitalsBlock,
    "",
    `### Long frames — ${snap.worstFrames.length}, ${Math.round(snap.totalBlockingMs)}ms blocking`,
    framesBlock,
    "",
    `### React commits — ${snap.commitCount} commits, ${Math.round(snap.commitTotalMs)}ms total, ${snap.longTaskCount} long tasks`,
    commitsBlock,
  ].join("\n")
}

function ratingColor(rating: string | undefined): string {
  if (rating === "poor") return "var(--danger)"
  if (rating === "needs-improvement") return "var(--warn)"
  if (rating === "good") return "var(--ok)"
  return "var(--muted-foreground)"
}

function Vital({
  label,
  value,
  rating,
  unit,
}: {
  label: string
  value: number | undefined
  rating: string | undefined
  unit?: string
}) {
  return (
    <div className="flex flex-1 flex-col rounded-lg border border-border/60 bg-card px-2 py-1.5">
      <span className="text-[9.5px] font-semibold uppercase tracking-wide text-muted-foreground/70">
        {label}
      </span>
      <span className="tabular-nums font-semibold" style={{ color: ratingColor(rating) }}>
        {value === undefined ? "—" : `${value}${unit ?? ""}`}
      </span>
    </div>
  )
}

function BlockRow({ block }: { block: BlockEvent }) {
  // A real main-thread block: sub-½s = amber hitch, multi-second = red freeze.
  const tone = block.blocked >= 1000 ? "var(--danger)" : "var(--warn)"
  const when = new Date(block.ts).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  })
  return (
    <div className="flex items-baseline gap-2">
      <span className="w-14 shrink-0 tabular-nums font-semibold" style={{ color: tone }}>
        {block.blocked}ms
      </span>
      <span className="truncate text-[11px] text-muted-foreground">blocked at {when}</span>
    </div>
  )
}

function AggRow({ agg }: { agg: TaskAgg }) {
  // Colour by SUMMED time — a burst of cheap ops turns red here even though no
  // single call is slow (that's the whole point of the aggregate view).
  const tone = agg.total >= 1000 ? "var(--danger)" : agg.total >= 250 ? "var(--warn)" : undefined
  return (
    <div className="flex items-baseline gap-2">
      <span
        className="w-16 shrink-0 tabular-nums font-semibold"
        style={tone ? { color: tone } : undefined}
      >
        {Math.round(agg.total)}ms
      </span>
      <span className="truncate font-mono text-[11px] text-muted-foreground" title={agg.label}>
        {agg.label}
      </span>
      <span className="ml-auto shrink-0 tabular-nums text-[10px] text-muted-foreground/60">
        ×{agg.count} · max {Math.round(agg.max)}ms
      </span>
    </div>
  )
}

function StallRow({ stall }: { stall: StallEvent }) {
  // Colour by severity: a sub-½s hitch is amber, a multi-second freeze is red.
  const tone = stall.gap >= 1000 ? "var(--danger)" : "var(--warn)"
  const when = new Date(stall.ts).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  })
  return (
    <div className="flex items-baseline gap-2">
      <span className="w-14 shrink-0 tabular-nums font-semibold" style={{ color: tone }}>
        {stall.gap}ms
      </span>
      <span className="truncate text-[11px] text-muted-foreground">blocked at {when}</span>
    </div>
  )
}

function TaskRow({ task }: { task: TaskEvent }) {
  // Colour by severity, mirroring the stall scale (a multi-second named block
  // is a red freeze, a sub-second one an amber hitch).
  const tone = task.duration >= 1000 ? "var(--danger)" : "var(--warn)"
  return (
    <div className="flex items-baseline gap-2">
      <span className="w-14 shrink-0 tabular-nums font-semibold" style={{ color: tone }}>
        {task.duration}ms
      </span>
      <span className="truncate font-mono text-[11px] text-muted-foreground" title={task.label}>
        {task.label}
      </span>
    </div>
  )
}

function FrameRow({ frame }: { frame: LoafEvent }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="w-12 shrink-0 tabular-nums font-medium text-[var(--warn)]">
        {frame.duration}ms
      </span>
      <span className="truncate text-[11px] text-muted-foreground" title={frame.script}>
        {frame.script ?? "(no script attribution)"}
      </span>
    </div>
  )
}

function CommitRow({ commit }: { commit: CommitEvent }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="w-12 shrink-0 tabular-nums font-medium text-[var(--signal)]">
        {commit.actualDuration}ms
      </span>
      <span className="truncate text-[11px] text-muted-foreground" title={commit.id}>
        {commit.id} · {commit.phase}
      </span>
    </div>
  )
}

function Group({
  title,
  meta,
  children,
}: {
  title: string
  meta: string
  children: React.ReactNode
}) {
  return (
    <section className="flex flex-col gap-1.5">
      <div className="flex items-baseline justify-between">
        <span className="text-[10.5px] font-semibold uppercase tracking-[0.06em] text-muted-foreground/80">
          {title}
        </span>
        <span className="text-[10px] tabular-nums text-muted-foreground/60">{meta}</span>
      </div>
      {children}
    </section>
  )
}

function Divider() {
  return <div className="h-px bg-border/60" />
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="text-[10.5px] italic text-muted-foreground/60">{children}</p>
}

function IconBtn({
  title,
  onClick,
  children,
}: {
  title: string
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      className={cn(
        "flex size-6 items-center justify-center rounded-md text-muted-foreground/70",
        "transition-colors hover:bg-muted/70 hover:text-foreground",
      )}
    >
      {children}
    </button>
  )
}
