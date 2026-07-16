import { Loader2 } from "lucide-react"
import type { ClaudeUsageLimit } from "@/lib/api/generated/types.gen"
import { cn } from "@/lib/utils"

/** Human label for a usage-limit `kind`. */
function limitLabel(kind: string): string {
  switch (kind) {
    case "session": {
      return "Session"
    }
    case "weekly_all": {
      return "Weekly (all)"
    }
    case "weekly_sonnet": {
      return "Sonnet"
    }
    case "weekly_opus": {
      return "Opus"
    }
    case "weekly_cowork": {
      return "Cowork"
    }
    default: {
      return kind.replaceAll("_", " ").replaceAll(/\b\w/g, (c) => c.toUpperCase())
    }
  }
}

/** Format a reset timestamp as a short relative string. */
function formatReset(iso: string | null | undefined): string {
  if (!iso) return ""
  const d = new Date(iso)
  const now = new Date()
  const diffMs = d.getTime() - now.getTime()
  if (diffMs < 0) return "resetting…"
  const diffH = diffMs / 3_600_000
  if (diffH < 24) {
    return `Resets ${d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`
  }
  return `Resets ${d.toLocaleDateString([], { month: "short", day: "numeric" })}`
}

/** Severity → bar colour. */
function barColor(severity: string, pct: number): string {
  if (severity === "critical" || pct >= 90) return "bg-red-500"
  if (severity === "warning" || pct >= 70) return "bg-amber-500"
  return "bg-emerald-500"
}

function LimitRow({ limit }: { limit: ClaudeUsageLimit }) {
  const pct = Math.min(limit.percent, 100)
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-[12px]">
        <span className="font-medium text-foreground">{limitLabel(limit.kind)}</span>
        <span className="text-muted-foreground tabular-nums">{pct}%</span>
      </div>
      <div className="h-1.5 w-full rounded-full bg-muted">
        <div
          className={cn(
            "h-full rounded-full transition-all duration-500",
            barColor(limit.severity, pct),
          )}
          style={{ width: `${pct}%` }}
        />
      </div>
      {limit.resets_at && (
        <p className="text-[11px] text-muted-foreground/70">{formatReset(limit.resets_at)}</p>
      )}
    </div>
  )
}

/** Usage-limit bars (loading/error/empty placeholder) — extracted for P8 budget. */
export function UsageLimits({
  isValid,
  isLoading,
  isError,
  limits,
}: {
  isValid: boolean
  isLoading: boolean
  isError: boolean
  limits: ClaudeUsageLimit[]
}) {
  if (!isValid) return null
  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-4">
        <Loader2 className="size-4 animate-spin text-muted-foreground" />
      </div>
    )
  }
  if (isError)
    return <p className="text-[12px] text-muted-foreground">Could not fetch usage data.</p>
  if (limits.length === 0) {
    return <p className="text-[12px] text-muted-foreground">No active usage limits.</p>
  }
  return (
    <>
      {limits.map((l) => (
        <LimitRow key={l.kind} limit={l} />
      ))}
    </>
  )
}
