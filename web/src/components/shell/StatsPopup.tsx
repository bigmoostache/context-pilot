import { useCallback, useMemo, useState } from "react"
import { Activity, Loader2, Stethoscope, X } from "lucide-react"
import { Dialog, DialogContent, DialogClose } from "@/components/ui/dialog"
import { usePanels, useAgentMeta } from "@/lib/live"
import { fetchVitals, type Vital } from "@/lib/api"
import { accentVar, fmtTokens } from "@/lib/panelMeta"
import type { StatRow } from "@/lib/types"

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

  const stats: StatRow[] = useMemo(() => [
    { label: "Context", value: `${fmtTokens(totalTokens)} / ${fmtTokens(budget)}`, accent: "signal" },
    { label: "Panels", value: String(panels.length) },
    { label: "Session cost", value: agent ? `$${agent.costUsd.toFixed(2)}` : "—", accent: "warn" },
  ], [totalTokens, panels.length, agent])

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

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[88vh] w-[360px] max-w-[calc(100vw-3rem)] flex-col">
        {/* header */}
        <div className="flex items-start gap-3 border-b border-border/70 bg-surface/60 px-5 py-4">
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
          {/* context budget meter */}
          <div className="flex flex-col gap-2 border-b border-border/70 px-5 py-4">
            <div className="flex items-baseline justify-between">
              <span className="text-[12px] text-muted-foreground">Context budget</span>
              <span className="font-mono text-[12px] tabular-nums text-foreground/85">
                {fmtTokens(totalTokens)} / {fmtTokens(budget)}
              </span>
            </div>
            <div className="relative h-1.5 overflow-hidden rounded-full bg-muted">
              <span
                className="absolute inset-y-0 left-0 rounded-full transition-[width]"
                style={{ width: `${pct}%`, background: "var(--signal)" }}
              />
              {/* threshold tick */}
              <span
                className="absolute inset-y-0 w-px bg-[var(--warn)]/70"
                style={{ left: `${(threshold / budget) * 100}%` }}
              />
            </div>
            <span className="text-[10.5px] text-muted-foreground/55">
              {pct}% used · cleaning threshold at{" "}
              {Math.round((threshold / budget) * 100)}%
            </span>
          </div>

          {/* stat rows */}
          <div className="flex flex-col px-5 py-2">
            {stats.map((s) => (
              <div
                key={s.label}
                className="flex items-center justify-between border-b border-border/40 py-2 last:border-0"
              >
                <span className="text-[12px] text-muted-foreground">{s.label}</span>
                <span
                  className="text-[12.5px] font-semibold tabular-nums"
                  style={{ color: s.accent ? accentVar[s.accent] : "var(--foreground)" }}
                >
                  {s.value}
                </span>
              </div>
            ))}
          </div>

          {/* check vitals */}
          <div className="flex flex-col gap-3 border-t border-border/70 px-5 py-4">
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 flex-col">
                <span className="text-[12px] font-medium text-foreground">Service vitals</span>
                <span className="text-[10.5px] text-muted-foreground/60">
                  Live connectivity across every dependency
                </span>
              </div>
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
                {checking ? "Checking…" : "Check Vitals"}
              </button>
            </div>

            {vitalsErr && (
              <div
                role="alert"
                className="rounded-lg border border-[var(--danger)]/30 bg-[var(--danger)]/10 px-3 py-2 text-[11.5px] text-[var(--danger)]"
              >
                {vitalsErr}
              </div>
            )}

            {vitals && (
              <ul className="flex flex-col gap-px overflow-hidden rounded-lg border border-border/60">
                {vitals.map((v, i) => (
                  <VitalRow key={`${v.name}-${i}`} vital={v} />
                ))}
              </ul>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** Map a vital status to its semantic theme color (green / red / grey). */
function statusColor(status: Vital["status"]): string {
  if (status === "ok") return "var(--ok)"
  if (status === "error") return "var(--danger)"
  return "var(--muted-foreground)"
}

/** One probe row: a status dot, the service name + honest detail line, and the
 *  measured latency (when the probe reports one). */
function VitalRow({ vital }: { vital: Vital }) {
  const color = statusColor(vital.status)
  return (
    <li className="flex items-center gap-2.5 bg-surface/40 px-3 py-2">
      <span
        className="size-2 shrink-0 rounded-full"
        style={{ background: color, boxShadow: `0 0 6px ${color}66` }}
        aria-hidden
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[12px] font-medium text-foreground">{vital.name}</span>
        {vital.detail && (
          <span className="truncate text-[10.5px] text-muted-foreground/65" title={vital.detail}>
            {vital.detail}
          </span>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {vital.latencyMs != null && (
          <span className="font-mono text-[10.5px] tabular-nums text-muted-foreground/70">
            {vital.latencyMs}ms
          </span>
        )}
        <span
          className="text-[10px] font-semibold uppercase tracking-[0.06em]"
          style={{ color }}
        >
          {vital.status}
        </span>
      </div>
    </li>
  )
}
