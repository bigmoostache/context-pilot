import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { Popover, PopoverTrigger, PopoverContent } from "@/components/ui/popover"
import { Tip } from "@/components/ui/tip"
import { fetchClaudeUsage } from "@/lib/api"
import type { ClaudeUsageLimit } from "@/lib/api/generated/types.gen"
import { cn } from "@/lib/utils"

/** Anthropic "A" logomark (Simple Icons, 24×24 viewBox). */
function AnthropicMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden>
      <path d="M13.827 3.52h3.603L24 20.48h-3.603l-6.57-16.96zm-7.257 0h3.603l6.57 16.96h-3.603L11.627 16.47H5.166L3.653 20.48H0L6.57 3.52zm1.04 4.96l-2.49 6.7h5.47l-2.49-6.7z" />
    </svg>
  )
}

/** Human label for a usage-limit `kind`. */
function limitLabel(kind: string): string {
  switch (kind) {
    case "session": return "Session"
    case "weekly_all": return "Weekly (all)"
    case "weekly_sonnet": return "Sonnet"
    case "weekly_opus": return "Opus"
    case "weekly_cowork": return "Cowork"
    default: return kind.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())
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
  const pct = Math.min(limit.percent ?? 0, 100)
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-[12px]">
        <span className="font-medium text-foreground">{limitLabel(limit.kind ?? "")}</span>
        <span className="tabular-nums text-muted-foreground">{pct}%</span>
      </div>
      <div className="h-1.5 w-full rounded-full bg-muted">
        <div
          className={cn("h-full rounded-full transition-all duration-500", barColor(limit.severity ?? "normal", pct))}
          style={{ width: `${pct}%` }}
        />
      </div>
      {limit.resets_at && (
        <p className="text-[11px] text-muted-foreground/70">{formatReset(limit.resets_at)}</p>
      )}
    </div>
  )
}

/** Anthropic logo button that opens a popover with live usage bars. */
export function UsageButton() {
  const [open, setOpen] = useState(false)

  const { data, isLoading, isError } = useQuery({
    queryKey: ["claude-usage"],
    queryFn: fetchClaudeUsage,
    enabled: open,
    refetchInterval: open ? 30_000 : false,
    staleTime: 10_000,
    retry: 1,
  })

  const limits = (data?.limits ?? []).filter((l) => l.percent != null && l.percent > 0)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Tip title="Usage limits" body="Claude Code session & weekly rate limits." side="bottom">
        <PopoverTrigger
          className="flex size-7 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground"
          aria-label="Claude Code usage"
        >
          <AnthropicMark className="size-[17px]" />
        </PopoverTrigger>
      </Tip>

      <PopoverContent side="bottom" align="end" sideOffset={8} className="w-64 space-y-3 p-4">
        <h4 className="text-[13px] font-semibold">Claude Code Usage</h4>

        {isLoading && (
          <div className="flex items-center justify-center py-4">
            <Loader2 className="size-4 animate-spin text-muted-foreground" />
          </div>
        )}

        {isError && (
          <p className="text-[12px] text-muted-foreground">
            Could not fetch usage — is Claude Code logged in?
          </p>
        )}

        {!isLoading && !isError && limits.length === 0 && (
          <p className="text-[12px] text-muted-foreground">No active usage limits.</p>
        )}

        {limits.map((l) => (
          <LimitRow key={l.kind} limit={l} />
        ))}
      </PopoverContent>
    </Popover>
  )
}
