import { useCallback, useMemo, useState } from "react"
import { CheckCircle2, Loader2, Stethoscope, TriangleAlert } from "lucide-react"
import { fetchVitals, type Vital } from "@/lib/api"

/**
 * Service vitals — on-demand connectivity board, as an **embeddable section**
 * (no dialog chrome of its own).
 *
 * Rendered inside the Agent Configuration dialog ({@link AgentModal} manage
 * mode) right column. The backend (`GET /api/agent/{id}/vitals`) runs the checks
 * it can reach; we prepend the two only the browser can observe (this app is
 * alive, and the measured round-trip), so the board covers the whole dependency
 * chain end to end. Healthy rows stay quiet (a coloured dot + latency); a
 * degraded/unreachable service raises a loud status chip so problems pop.
 */
export function SessionVitals({ agentId }: { agentId: string }) {
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
        {
          name: "Frontend → Orchestrator",
          category: "frontend",
          status: "ok",
          latencyMs: rtt,
          detail: `round-trip ${rtt}ms`,
        },
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
    <div className="flex flex-col">
      {/* ── service grid band ── */}
      <div className="flex flex-col gap-3.5">
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
              onClick={() => void runChecks()}
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
            className="rounded-lg border border-(--danger)/30 bg-(--danger)/10 px-3 py-2 text-[11.5px] text-(--danger)"
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
            <span className="text-[10px] font-semibold tracking-[0.07em] text-muted-foreground/55 uppercase">
              {g.label}
            </span>
            <ul className="grid grid-cols-1 gap-1.5 sm:grid-cols-2">
              {g.rows.map((v) => (
                <VitalRow key={v.name} vital={v} />
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
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
  return s.replaceAll(/[-_]+/g, " ").replaceAll(/\b\w/g, (c) => c.toUpperCase())
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
        <span className="font-mono text-[10.5px] text-muted-foreground/65 tabular-nums">
          {vital.latencyMs}ms
        </span>
      )}
      {!healthy && (
        <span
          className="shrink-0 rounded-sm px-1.5 py-0.5 text-[9.5px] font-semibold tracking-wider uppercase"
          style={{ color, background: `color-mix(in oklab, ${color} 14%, transparent)` }}
        >
          {vital.status}
        </span>
      )}
    </li>
  )
}
