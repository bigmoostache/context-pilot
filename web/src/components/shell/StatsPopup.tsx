import { useMemo } from "react"
import { Activity, X } from "lucide-react"
import { Dialog, DialogContent, DialogClose } from "@/components/ui/dialog"
import { usePanels, useAgentMeta } from "@/lib/live"
import { accentVar, fmtTokens } from "@/lib/panelMeta"
import type { StatRow } from "@/lib/types"

/**
 * Session "vitals" popup — the stats that used to live in the cockpit's right
 * rail, now summoned on demand from a button in the global header. Surfacing
 * them this way keeps the cockpit a clean three-column reading surface while
 * the figures stay one click away from every view.
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

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="w-[360px] max-w-[calc(100vw-3rem)]">
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
      </DialogContent>
    </Dialog>
  )
}
