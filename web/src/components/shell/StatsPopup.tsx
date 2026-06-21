import { useCallback, useMemo, useState } from "react"
import { Activity, CheckCircle2, Loader2, Stethoscope, TriangleAlert, X } from "lucide-react"
import { Dialog, DialogContent, DialogClose } from "@/components/ui/dialog"
import { usePanels, useAgentMeta } from "@/lib/live"
import { fetchVitals, type Vital } from "@/lib/api"
import { accentVar, fmtTokens } from "@/lib/support/panelMeta"

/**
 * Session "vitals" popup — the stats that used to live in the cockpit's right
 * rail, now summoned on demand from a button in the global header. Surfacing
 * them this way keeps the cockpit a clean three-column reading surface while
 * the figures stay one click away from every view.
 *
 * Beyond the static budget/cost figures it also hosts the **Check Vitals**
 * action: an on-demand (never polling) probe of every service the agent
 * depends on. The backend (`GET /api/agent/{id}/vitals`) runs the real
 * connectivity checks it can reach; this component prepends the two checks only
 * the browser can observe — that the frontend itself is alive, and the measured
 * round-trip latency of the probe request — so the rendered table covers the
 * whole dependency chain end to end.
 *
 * Layout (T269): a wide two-band sheet. The top band is the session summary
 * (context-budget meter + three figure chips); the bottom band is the service
 * grid — probe results grouped by category and laid out two-per-row so a dozen
 * dependencies read as a compact, scannable board rather than a long stack.
 * Healthy rows stay quiet (a coloured dot + latency); only a degraded or
 * unreachable service raises a loud status chip, so problems pop.
 *
 * Built on the portaled {@link Dialog} primitive (renders into `document.body`,
 * focus-trapped, Esc / click-out to dismiss) — same reasoning as the thread
 * dossier and settings sheet: a hand-rolled `fixed` overlay can be trapped by a
 * transformed/blurred ancestor's containing block (the TopBar `.vibrancy` blur).
 */
export function StatsPopup({
  open,
  onClose,
  agentId,
}: {
  open: boolean
  onClose: () => void
  agentId: string
}) {
  const { data: panels = [] } = usePanels(agentId)
  const { data: agent } = useAgentMeta(agentId)
  const totalTokens = useMemo(() => panels.reduce((s, p) => s + p.tokens, 0), [panels])
  const budget = 200_000
  const threshold = 170_000
  const pct = Math.round((totalTokens / budget) * 100)

  const stats = useMemo(
    () => [
      {
        label: "Context",
        value: `${fmtTokens(totalTokens)} / ${fmtTokens(budget)}`,
        accent: "signal" as const,
      },
      { label: "Panels", value: String(panels.length), accent: undefined },
      {
        label: "Session cost",
        value: agent ? `$${agent.costUsd.toFixed(2)}` : "—",
        accent: "warn" as const,
      },
    ],
    [totalTokens, panels.length, agent],
  )

  // ── Check Vitals (on-demand connectivity probe) ────────────────────
  const [vitals, setVitals] = useState<Vital[] | null>(null)
  const [checking, setChecking] = useState(false)
  const [vitalsErr, setVitalsErr] = useState<string | null>(null)

  const runChecks = useCallback(async () => {
    setChecking(true)
    setVitalsErr(null)
    const t0 = performance.now()
    try {
      const rows = await fetchVitals(agentId)
      const rtt = Math.round(performance.now() - t0)
      // The two checks only the browser can observe: that this app is running
      // (we are executing) and the measured round-trip to the orchestrator.
      const frontendRows: Vital[] = [
        { name: "Frontend", category: "frontend", status: "ok", detail: "this app is running" },
        { name: "Frontend → Orchestrator", category: "frontend", status: "ok", latencyMs: rtt, detail: `round-trip ${rtt}ms` },
      ]
      setVitals([...frontendRows, ...rows])
    } catch (e) {
      setVitalsErr(e instanceof Error ? e.message : "vitals check failed")
      setVitals(null)
    } finally {
      setChecking(false)
    }
  }, [agentId])

  // Group the flat probe list by category (first-seen order preserved) so the
  // grid reads as labelled sections rather than a homogeneous wall of rows.
  const groups = useMemo(() => groupByCategory(vitals), [vitals])
  const summary = useMemo(() => {
    if (!vitals) return null
    const issues = vitals.filter((v) => v.status !== "ok").length
    return { total: vitals.length, ok: vitals.length - issues, issues }
  }, [vitals])

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[88vh] w-[760px] max-w-[calc(100vw-3rem)] flex-col">
        {/* header */}
        <div className="flex items-start gap-3 border-b border-border/70 bg-surface/60 px-6 py-4">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-xl bg-[var(--signal)]/14 text-[var(--signal)] ring-1 ring-inset ring-[var(--signal)]/25">
            <Activity className="size-[18px]" />
          </span>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 pt-0.5">
            <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-muted-foreground/70">
              Session vitals
            </span>
            <span className="truncate text-[15px] font-semibold tracking-tight text-foreground">
              Context Pilot
            </span>
          </div>
          <DialogClose
            className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
            aria-label="Close"
          >
            <X className="size-4" />
          </DialogClose>
        </div>

        {/* scrollable body */}
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          {/* ── session summary band ── */}
          <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-8 gap-y-4 border-b border-border/70 px-6 py-5">
            {/* budget meter */}
            <div className="flex flex-col gap-2">
              <div className="flex items-baseline justify-between">
                <span className="text-[11.5px] text-muted-foreground">Context budget</span>
                <span className="font-mono text-[11.5px] tabular-nums text-foreground/85">
                  {fmtTokens(totalTokens)} / {fmtTokens(budget)} · {pct}%
                </span>
              </div>
              <div className="relative h-1.5 overflow-hidden rounded-full bg-muted">
                <span
                  className="absolute inset-y-0 left-0 rounded-full transition-[width]"
                  style={{ width: `${pct}%`, background: "var(--signal)" }}
                />
                <span
                  className="absolute inset-y-0 w-px bg-[var(--warn)]/70"
                  style={{ left: `${(threshold / budget) * 100}%` }}
                />
              </div>
              <span className="text-[10px] text-muted-foreground/55">
                cleaning threshold at {Math.round((threshold / budget) * 100)}%
              </span>
            </div>

            {/* figure chips */}
            <div className="flex items-stretch gap-2.5">
              {stats.map((s) => (
                <div
                  key={s.label}
                  className="flex min-w-[104px] flex-col gap-0.5 rounded-lg border border-border/60 bg-surface/50 px-3 py-2"
                >
                  <span className="text-[10px] uppercase tracking-[0.05em] text-muted-foreground/65">
                    {s.label}
                  </span>
                  <span
                    className="text-[14px] font-semibold tabular-nums"
                    style={{ color: s.accent ? accentVar[s.accent] : "var(--foreground)" }}
                  >
                    {s.value}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* ── service grid band ── */}
          <div className="flex flex-col gap-3.5 px-6 py-5">
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 flex-col">
                <span className="text-[12.5px] font-medium text-foreground">Service vitals</span>
                <span className="text-[10.5px] text-muted-foreground/60">
                  {summary
                    ? `${summary.ok} of ${summary.total} reachable`
                    : "Live connectivity across every dependency"}
                </span>
              </div>
              <div className="flex items-center gap-2.5">
                {summary &&
                  (summary.issues === 0 ? (
                    <span
                      className="flex items-center gap-1 text-[11px] font-medium"
                      style={{ color: "var(--ok)" }}
                    >
                      <CheckCircle2 className="size-3.5" /> all healthy
                    </span>
                  ) : (
                    <span
                      className="flex items-center gap-1 text-[11px] font-medium"
                      style={{ color: "var(--warn)" }}
                    >
                      <TriangleAlert className="size-3.5" /> {summary.issues}{" "}
                      {summary.issues === 1 ? "issue" : "issues"}
                    </span>
                  ))}
                <button
                  type="button"
                  onClick={runChecks}
                  disabled={checking}
                  className="flex shrink-0 items-center gap-1.5 rounded-lg border border-border bg-surface/70 px-3 py-1.5 text-[12px] font-semibold text-foreground transition-colors hover:bg-muted disabled:opacity-60"
                >
                  {checking ? (
                    <Loader2 className="size-3.5 animate-spin" />
                  ) : (
                    <Stethoscope className="size-3.5" />
                  )}
                  {checking ? "Checking…" : summary ? "Re-check" : "Check Vitals"}
                </button>
              </div>
            </div>

            {vitalsErr && (
              <div
                role="alert"
                className="rounded-lg border border-[var(--danger)]/30 bg-[var(--danger)]/10 px-3 py-2 text-[11.5px] text-[var(--danger)]"
              >
                {vitalsErr}
              </div>
            )}

            {!vitals && !vitalsErr && (
              <div className="flex flex-col items-center justify-center gap-1.5 rounded-xl border border-dashed border-border/60 py-9 text-center">
                <Stethoscope className="size-5 text-muted-foreground/40" />
                <span className="text-[11.5px] text-muted-foreground/60">
                  Run a check to probe every service connection.
                </span>
              </div>
            )}

            {groups.map((g) => (
              <section key={g.category} className="flex flex-col gap-1.5">
                <span className="text-[10px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/55">
                  {g.label}
                </span>
                <ul className="grid grid-cols-1 gap-1.5 sm:grid-cols-2">
                  {g.rows.map((v, i) => (
                    <VitalRow key={`${v.name}-${i}`} vital={v} />
                  ))}
                </ul>
              </section>
            ))}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** A category section of probe rows, label humanised from the backend's tag. */
interface VitalGroup {
  category: string
  label: string
  rows: Vital[]
}

/** Bucket the flat probe list by `category`, preserving first-seen order so the
 *  frontend rows lead and the rest follow the backend's natural ordering. */
function groupByCategory(vitals: Vital[] | null): VitalGroup[] {
  if (!vitals) return []
  const order: string[] = []
  const byCat = new Map<string, Vital[]>()
  for (const v of vitals) {
    const cat = v.category || "other"
    if (!byCat.has(cat)) {
      byCat.set(cat, [])
      order.push(cat)
    }
    byCat.get(cat)?.push(v)
  }
  return order.map((category) => ({
    category,
    label: humanize(category),
    rows: byCat.get(category) ?? [],
  }))
}

/** Turn a snake/kebab category tag into a Title-Cased section label. */
function humanize(s: string): string {
  return s
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase())
}

/** Map a vital status to its semantic theme color (green / red / grey). */
function statusColor(status: Vital["status"]): string {
  if (status === "ok") return "var(--ok)"
  if (status === "error") return "var(--danger)"
  return "var(--muted-foreground)"
}

/** One compact probe cell: a status dot, the service name, the measured latency
 *  (when reported), and — only when the service is NOT ok — a loud status chip,
 *  so a healthy board stays quiet and a problem leaps out. The honest detail
 *  line rides the `title` tooltip to keep the cell tight. */
function VitalRow({ vital }: { vital: Vital }) {
  const color = statusColor(vital.status)
  const healthy = vital.status === "ok"
  return (
    <li
      className="flex items-center gap-2.5 rounded-lg border border-border/50 bg-surface/40 px-3 py-2"
      title={vital.detail}
    >
      <span
        className="size-2 shrink-0 rounded-full"
        style={{ background: color, boxShadow: `0 0 6px ${color}66` }}
        aria-hidden
      />
      <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-foreground">
        {vital.name}
      </span>
      {vital.latencyMs != null && (
        <span className="font-mono text-[10.5px] tabular-nums text-muted-foreground/65">
          {vital.latencyMs}ms
        </span>
      )}
      {!healthy && (
        <span
          className="shrink-0 rounded px-1.5 py-0.5 text-[9.5px] font-semibold uppercase tracking-[0.05em]"
          style={{ color, background: `color-mix(in oklab, ${color} 14%, transparent)` }}
        >
          {vital.status}
        </span>
      )}
    </li>
  )
}
